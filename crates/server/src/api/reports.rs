//! Game reports (FR-RP-01 to 05, FR-RP-09, SEC-10, SEC-23).
//!
//! - `GET /sessions?page=`: the host's past games, newest first.
//! - `GET /sessions/:id`: summary, per-question figures and every player's
//!   answers.
//! - `POST /sessions/:id/exports { format }`: a signed link valid 10 min to
//!   a CSV or Excel file, served by `GET /exports/:token`.
//! - `DELETE /sessions/:id`: removes the report for good.
//!
//! Everything is built from the session's quiz snapshot, so editing or
//! deleting the quiz later never changes a report.

use std::collections::HashMap;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use myquiz_engine::quiz::QuizSnapshot;
use myquiz_engine::QuestionKind;
use myquiz_protocol::quiz::ExportLink;
use myquiz_protocol::report::{
    PlayerAnswer, PlayerResult, QuestionStat, RenameSession, ReportExport, ReportFormat,
    SessionPage, SessionReport, SessionSummary,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use super::audit::{audit, AuditEntry};
use super::auth::AuthUser;
use super::error::{ApiResult, AppError};
use super::quizzes::rfc3339;
use super::{signed, ApiState};

const PAGE_SIZE: u32 = 20;
/// FR-RP-02: questions under this accuracy are flagged as hard.
const HARD_BELOW: f64 = 0.35;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    page: Option<u32>,
    /// Filters by game name or quiz title (case-insensitive substring).
    q: Option<String>,
}

/// Longest game name, in characters.
pub const NAME_MAX: usize = 80;

/// Trims a game name; blank becomes None. Err when it is too long.
pub fn clean_name(name: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(n) = name.map(str::trim).filter(|n| !n.is_empty()) else {
        return Ok(None);
    };
    if n.chars().count() > NAME_MAX {
        return Err(AppError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "NAME_LENGTH",
            "the game name can be at most 80 characters",
        ));
    }
    Ok(Some(n.to_string()))
}

fn parse_id(id: &str) -> ApiResult<Uuid> {
    id.parse()
        .map_err(|_| AppError::not_found("SESSION_NOT_FOUND"))
}

/// The host, or an admin, may read a report; anyone else gets 404 so
/// session ids cannot be probed (SEC-23).
async fn check_access(db: &PgPool, user: &AuthUser, id: Uuid) -> ApiResult<()> {
    let host: Option<Uuid> = sqlx::query_scalar("SELECT host_id FROM game_sessions WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await?;
    match host {
        Some(h) if h == user.id || user.is_admin() => Ok(()),
        _ => Err(AppError::not_found("SESSION_NOT_FOUND")),
    }
}

/// `GET /sessions?page=` (FR-RP-01). Only games that started are listed.
pub async fn list(
    State(state): State<ApiState>,
    user: AuthUser,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<SessionPage>> {
    let page = query.page.unwrap_or(1).clamp(1, 10_000);
    let search = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(|q| {
            let escaped = q
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            format!("%{escaped}%")
        });
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        Uuid,
        Option<String>,
        String,
        OffsetDateTime,
        Option<OffsetDateTime>,
        bool,
        i64,
        f64,
        i64,
    )> = sqlx::query_as(
        "SELECT s.id, s.name, s.quiz_snapshot->>'title', s.started_at, s.ended_at, s.status = 'ended', \
                (SELECT count(*) FROM session_players p WHERE p.session_id = s.id AND NOT p.kicked), \
                (SELECT COALESCE(avg(COALESCE(p.final_score, \
                     (SELECT COALESCE(sum(a.points), 0) FROM answers a WHERE a.player_id = p.id))), 0)::float8 \
                   FROM session_players p WHERE p.session_id = s.id AND NOT p.kicked), \
                count(*) OVER () \
         FROM game_sessions s \
         WHERE s.host_id = $1 AND s.started_at IS NOT NULL \
           AND ($4::text IS NULL OR s.name ILIKE $4 OR s.quiz_snapshot->>'title' ILIKE $4) \
         ORDER BY s.started_at DESC, s.id \
         LIMIT $2 OFFSET $3",
    )
    .bind(user.id)
    .bind(PAGE_SIZE as i64)
    .bind(((page - 1) * PAGE_SIZE) as i64)
    .bind(&search)
    .fetch_all(&state.app.db)
    .await?;
    let total =
        match rows.first() {
            Some(r) => r.8,
            None if page == 1 => 0,
            None => sqlx::query_scalar(
                "SELECT count(*) FROM game_sessions s WHERE s.host_id = $1 AND s.started_at IS NOT NULL \
                   AND ($2::text IS NULL OR s.name ILIKE $2 OR s.quiz_snapshot->>'title' ILIKE $2)",
            )
            .bind(user.id)
            .bind(&search)
            .fetch_one(&state.app.db)
            .await?,
        };
    Ok(Json(SessionPage {
        items: rows
            .into_iter()
            .map(|r| SessionSummary {
                id: r.0.to_string(),
                name: r.1,
                quiz_title: r.2,
                started_at: rfc3339(r.3),
                ended_at: r.4.map(rfc3339),
                ended: r.5,
                player_count: r.6 as u32,
                average_score: round1(r.7),
            })
            .collect(),
        page,
        page_size: PAGE_SIZE,
        total,
    }))
}

/// `GET /sessions/:id` (FR-RP-02, FR-RP-03).
pub async fn get(
    State(state): State<ApiState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<SessionReport>> {
    let id = parse_id(&id)?;
    check_access(&state.app.db, &user, id).await?;
    Ok(Json(build(&state.app.db, id).await?.0))
}

/// `PATCH /sessions/:id { name }`: the host names or renames a game, from
/// the lobby or later from its report. Blank clears the name.
pub async fn rename(
    State(state): State<ApiState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<RenameSession>,
) -> ApiResult<Json<SessionSummary>> {
    let id = parse_id(&id)?;
    let name = clean_name(body.name.as_deref())?;
    let done = sqlx::query(
        "UPDATE game_sessions SET name = $3 WHERE id = $1 AND host_id = $2 RETURNING id",
    )
    .bind(id)
    .bind(user.id)
    .bind(&name)
    .fetch_optional(&state.app.db)
    .await?;
    if done.is_none() {
        return Err(AppError::not_found("SESSION_NOT_FOUND"));
    }
    Ok(Json(build(&state.app.db, id).await?.0.session))
}

/// `DELETE /sessions/:id` (FR-RP-09): the host removes a report for good.
/// Answers and players go with it (ON DELETE CASCADE).
pub async fn delete(
    State(state): State<ApiState>,
    user: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    let id = parse_id(&id)?;
    // Only the host: admins can read reports but not delete them.
    let done = sqlx::query(
        "DELETE FROM game_sessions WHERE id = $1 AND host_id = $2 AND status <> 'running' \
         RETURNING id",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&state.app.db)
    .await?;
    if done.is_none() {
        return Err(AppError::not_found("SESSION_NOT_FOUND"));
    }
    audit(
        &state.app.db,
        &state.auth.app_secret,
        &headers,
        AuditEntry {
            actor: Some(user.id),
            action: "report_deleted",
            target_type: Some("session"),
            target_id: Some(id.to_string()),
            details: json!({}),
        },
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /sessions/:id/exports { format }` (FR-RP-04, FR-RP-05, SEC-23).
pub async fn export(
    State(state): State<ApiState>,
    user: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ReportExport>,
) -> ApiResult<Json<ExportLink>> {
    let id = parse_id(&id)?;
    check_access(&state.app.db, &user, id).await?;
    let format = format_name(body.format);
    let (token, exp) = signed::sign(&state.auth.app_secret, &format!("report:{id}:{format}"));
    audit(
        &state.app.db,
        &state.auth.app_secret,
        &headers,
        AuditEntry {
            actor: Some(user.id),
            action: "report_exported",
            target_type: Some("session"),
            target_id: Some(id.to_string()),
            details: json!({ "format": format }),
        },
    )
    .await;
    Ok(Json(ExportLink {
        url: format!("/api/v1/exports/{token}"),
        expires_at: rfc3339(OffsetDateTime::from_unix_timestamp(exp as i64)?),
    }))
}

fn format_name(f: ReportFormat) -> &'static str {
    match f {
        ReportFormat::Csv => "csv",
        ReportFormat::CsvSummary => "csv_summary",
        ReportFormat::Xlsx => "xlsx",
    }
}

/// Serves a verified `report:<id>:<format>` link subject.
pub async fn serve(db: &PgPool, subject: &str) -> ApiResult<Option<Response>> {
    let Some(rest) = subject.strip_prefix("report:") else {
        return Ok(None);
    };
    let Some((id, format)) = rest.split_once(':') else {
        return Ok(None);
    };
    let id: Uuid = id
        .parse()
        .map_err(|_| AppError::not_found("SESSION_NOT_FOUND"))?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM game_sessions WHERE id = $1)")
            .bind(id)
            .fetch_one(db)
            .await?;
    if !exists {
        return Err(AppError::not_found("SESSION_NOT_FOUND"));
    }
    let (report, snapshot) = build(db, id).await?;
    let stem = file_stem(&report.session);
    let (bytes, mime, ext, name_suffix) = match format {
        "csv" => (
            answers_csv(&report, &snapshot)?,
            "text/csv; charset=utf-8",
            "csv",
            "",
        ),
        "csv_summary" => (
            players_csv(&report)?,
            "text/csv; charset=utf-8",
            "csv",
            "-summary",
        ),
        "xlsx" => (
            xlsx(&report, &snapshot)?,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "xlsx",
            "",
        ),
        _ => return Ok(None),
    };
    let mut res = Body::from(bytes).into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{stem}{name_suffix}.{ext}\""
        ))?,
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(Some(res))
}

// ---------------------------------------------------------------------------
// Building the report

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn ratio(n: u32, d: u32) -> f64 {
    if d == 0 {
        0.0
    } else {
        (n as f64 / d as f64 * 1000.0).round() / 1000.0
    }
}

/// `myquiz-report-<title slug>-<yyyymmdd>`, ASCII only so it is safe in a
/// header.
fn file_stem(s: &SessionSummary) -> String {
    let mut slug = String::new();
    let label = match &s.name {
        Some(n) => format!("{n} {}", s.quiz_title),
        None => s.quiz_title.clone(),
    };
    for c in myquiz_engine::matching::normalize(&label, true).chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(60).collect();
    let slug = slug.trim_end_matches('-').to_string();
    let date: String = s
        .started_at
        .chars()
        .take(10)
        .filter(|c| *c != '-')
        .collect();
    if slug.is_empty() {
        format!("myquiz-report-{date}")
    } else {
        format!("myquiz-report-{slug}-{date}")
    }
}

struct AnswerRow {
    player: Uuid,
    index: i32,
    option_ids: Option<Vec<i32>>,
    text: Option<String>,
    correct: bool,
    response_ms: i32,
    points: i32,
}

async fn build(db: &PgPool, id: Uuid) -> ApiResult<(SessionReport, QuizSnapshot)> {
    #[allow(clippy::type_complexity)]
    let (snapshot, created, started, ended_at, ended, name): (
        serde_json::Value,
        OffsetDateTime,
        Option<OffsetDateTime>,
        Option<OffsetDateTime>,
        bool,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT quiz_snapshot, created_at, started_at, ended_at, status = 'ended', name \
         FROM game_sessions WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::not_found("SESSION_NOT_FOUND"))?;
    let snapshot: QuizSnapshot = serde_json::from_value(snapshot)?;

    #[allow(clippy::type_complexity)]
    let players: Vec<(Uuid, String, Option<i32>, Option<i32>, bool, i32)> = sqlx::query_as(
        "SELECT id, nickname, final_score, final_rank, kicked, join_order \
         FROM session_players WHERE session_id = $1",
    )
    .bind(id)
    .fetch_all(db)
    .await?;
    #[allow(clippy::type_complexity)]
    let answers: Vec<(Uuid, i32, Option<Vec<i32>>, Option<String>, bool, i32, i32)> =
        sqlx::query_as(
            "SELECT player_id, question_index, option_ids, text_answer, is_correct, \
                    response_ms, points \
             FROM answers WHERE session_id = $1",
        )
        .bind(id)
        .fetch_all(db)
        .await?;
    let answers: Vec<AnswerRow> = answers
        .into_iter()
        .map(|a| AnswerRow {
            player: a.0,
            index: a.1,
            option_ids: a.2,
            text: a.3,
            correct: a.4,
            response_ms: a.5,
            points: a.6,
        })
        .collect();

    let n_questions = snapshot.questions.len();
    let mut by_player: HashMap<Uuid, Vec<Option<&AnswerRow>>> = HashMap::new();
    for a in &answers {
        if a.index < 0 || a.index as usize >= n_questions {
            continue;
        }
        by_player
            .entry(a.player)
            .or_insert_with(|| vec![None; n_questions])[a.index as usize] = Some(a);
    }

    let mut results: Vec<(PlayerResult, i32)> = players
        .iter()
        .map(|(pid, nick, final_score, final_rank, kicked, join_order)| {
            let row = by_player.get(pid);
            let answers: Vec<PlayerAnswer> = (0..n_questions)
                .map(|i| {
                    let a = row.and_then(|r| r[i]);
                    PlayerAnswer {
                        question_index: i as u32,
                        answer: a.map(|a| answer_text(&snapshot, i, a)),
                        correct: a.is_some_and(|a| a.correct),
                        points: a.map_or(0, |a| a.points.max(0) as u32),
                        response_ms: a.map(|a| a.response_ms.max(0) as u32),
                    }
                })
                .collect();
            let summed: u64 = answers.iter().map(|a| a.points as u64).sum();
            (
                PlayerResult {
                    id: pid.to_string(),
                    nickname: nick.clone(),
                    rank: if *kicked {
                        None
                    } else {
                        final_rank.map(|r| r.max(0) as u32)
                    },
                    score: final_score.map_or(summed, |s| s.max(0) as u64),
                    correct_count: answers.iter().filter(|a| a.correct).count() as u32,
                    kicked: *kicked,
                    answers,
                },
                *join_order,
            )
        })
        .collect();

    // A game that never reached the end has no final ranks: rank by score
    // here, level scores sharing a rank.
    if results.iter().any(|(r, _)| !r.kicked && r.rank.is_none()) {
        let mut order: Vec<usize> = (0..results.len())
            .filter(|&i| !results[i].0.kicked)
            .collect();
        order.sort_by(|&a, &b| {
            results[b]
                .0
                .score
                .cmp(&results[a].0.score)
                .then(results[a].1.cmp(&results[b].1))
        });
        let mut prev: Option<(u64, u32)> = None;
        for (pos, &i) in order.iter().enumerate() {
            let score = results[i].0.score;
            let rank = match prev {
                Some((s, r)) if s == score => r,
                _ => pos as u32 + 1,
            };
            results[i].0.rank = Some(rank);
            prev = Some((score, rank));
        }
    }
    results.sort_by(|(a, ja), (b, jb)| {
        a.kicked
            .cmp(&b.kicked)
            .then(a.rank.unwrap_or(u32::MAX).cmp(&b.rank.unwrap_or(u32::MAX)))
            .then(ja.cmp(jb))
    });
    let players: Vec<PlayerResult> = results.into_iter().map(|(r, _)| r).collect();
    let in_game: Vec<&PlayerResult> = players.iter().filter(|p| !p.kicked).collect();
    let n_players = in_game.len() as u32;

    let questions: Vec<QuestionStat> = snapshot
        .questions
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let sent: Vec<&PlayerAnswer> = in_game
                .iter()
                .map(|p| &p.answers[i])
                .filter(|a| a.response_ms.is_some())
                .collect();
            let correct = sent.iter().filter(|a| a.correct).count() as u32;
            let accuracy = ratio(correct, n_players);
            QuestionStat {
                index: i as u32,
                kind: q.kind,
                text: q.text.clone(),
                answered: sent.len() as u32,
                correct,
                accuracy,
                average_ms: average(sent.iter().filter_map(|a| a.response_ms)),
                hard: n_players > 0 && accuracy < HARD_BELOW,
            }
        })
        .collect();

    let participants = in_game
        .iter()
        .filter(|p| p.answers.iter().any(|a| a.response_ms.is_some()))
        .count() as u32;
    let total_correct: u32 = in_game.iter().map(|p| p.correct_count).sum();
    let average_score = if in_game.is_empty() {
        0.0
    } else {
        in_game.iter().map(|p| p.score as f64).sum::<f64>() / in_game.len() as f64
    };
    let report = SessionReport {
        session: SessionSummary {
            id: id.to_string(),
            name,
            quiz_title: snapshot.title.clone(),
            started_at: rfc3339(started.unwrap_or(created)),
            ended_at: ended_at.map(rfc3339),
            ended,
            player_count: n_players,
            average_score: round1(average_score),
        },
        participation: ratio(participants, n_players),
        accuracy: ratio(total_correct, n_players * n_questions as u32),
        average_ms: average(
            in_game
                .iter()
                .flat_map(|p| p.answers.iter().filter_map(|a| a.response_ms)),
        ),
        questions,
        players,
    };
    Ok((report, snapshot))
}

fn average(xs: impl Iterator<Item = u32>) -> Option<u32> {
    let (sum, n) = xs.fold((0u64, 0u64), |(s, n), x| (s + x as u64, n + 1));
    (n > 0).then(|| (sum / n) as u32)
}

fn answer_text(snapshot: &QuizSnapshot, index: usize, a: &AnswerRow) -> String {
    let q = &snapshot.questions[index];
    if q.kind == QuestionKind::FillBlank {
        return a.text.clone().unwrap_or_default();
    }
    a.option_ids
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|id| {
            q.options
                .iter()
                .find(|o| o.id as i32 == *id)
                .map_or_else(|| format!("#{id}"), |o| o.text.clone())
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn correct_text(snapshot: &QuizSnapshot, index: usize) -> String {
    let q = &snapshot.questions[index];
    if q.kind == QuestionKind::FillBlank {
        return q.accepted_answers.join("; ");
    }
    q.options
        .iter()
        .filter(|o| o.is_correct)
        .map(|o| o.text.clone())
        .collect::<Vec<_>>()
        .join("; ")
}

fn kind_name(k: QuestionKind) -> &'static str {
    match k {
        QuestionKind::Single => "single",
        QuestionKind::Multi => "multi",
        QuestionKind::TrueFalse => "true_false",
        QuestionKind::FillBlank => "fill_blank",
    }
}

// ---------------------------------------------------------------------------
// Files

/// SEC-10: a cell that a spreadsheet could read as a formula gets a leading
/// apostrophe. Used by both the CSV and the Excel writer.
pub fn safe_cell(s: &str) -> String {
    match s.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r' | '−') => format!("'{s}"),
        _ => s.to_string(),
    }
}

fn seconds(ms: Option<u32>) -> String {
    ms.map(|ms| format!("{:.1}", ms as f64 / 1000.0))
        .unwrap_or_default()
}

/// UTF-8 with a BOM so Excel shows Vietnamese correctly (FR-RP-04).
fn csv_bytes(rows: Vec<Vec<String>>) -> ApiResult<Vec<u8>> {
    let mut w = csv::Writer::from_writer(vec![0xEF, 0xBB, 0xBF]);
    for row in rows {
        w.write_record(row.iter().map(|c| safe_cell(c)))?;
    }
    Ok(w.into_inner().map_err(|e| anyhow::anyhow!("{e}"))?)
}

const ANSWER_HEADERS: [&str; 11] = [
    "rank",
    "nickname",
    "question",
    "question_text",
    "type",
    "answer",
    "correct_answer",
    "is_correct",
    "response_s",
    "points",
    "total_score",
];

fn answer_rows(r: &SessionReport, snapshot: &QuizSnapshot) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for p in r.players.iter().filter(|p| !p.kicked) {
        for a in &p.answers {
            let i = a.question_index as usize;
            rows.push(vec![
                p.rank.map(|x| x.to_string()).unwrap_or_default(),
                p.nickname.clone(),
                (i + 1).to_string(),
                snapshot.questions[i].text.clone(),
                kind_name(snapshot.questions[i].kind).into(),
                a.answer.clone().unwrap_or_default(),
                correct_text(snapshot, i),
                if a.answer.is_none() {
                    "no answer".into()
                } else if a.correct {
                    "yes".into()
                } else {
                    "no".into()
                },
                seconds(a.response_ms),
                a.points.to_string(),
                p.score.to_string(),
            ]);
        }
    }
    rows
}

const PLAYER_HEADERS: [&str; 6] = [
    "rank",
    "nickname",
    "score",
    "correct",
    "questions",
    "average_response_s",
];

fn player_rows(r: &SessionReport) -> Vec<Vec<String>> {
    r.players
        .iter()
        .filter(|p| !p.kicked)
        .map(|p| {
            vec![
                p.rank.map(|x| x.to_string()).unwrap_or_default(),
                p.nickname.clone(),
                p.score.to_string(),
                p.correct_count.to_string(),
                p.answers.len().to_string(),
                seconds(average(p.answers.iter().filter_map(|a| a.response_ms))),
            ]
        })
        .collect()
}

fn answers_csv(r: &SessionReport, snapshot: &QuizSnapshot) -> ApiResult<Vec<u8>> {
    let mut rows = vec![ANSWER_HEADERS.iter().map(|s| s.to_string()).collect()];
    rows.extend(answer_rows(r, snapshot));
    csv_bytes(rows)
}

fn players_csv(r: &SessionReport) -> ApiResult<Vec<u8>> {
    let mut rows = vec![PLAYER_HEADERS.iter().map(|s| s.to_string()).collect()];
    rows.extend(player_rows(r));
    csv_bytes(rows)
}

/// Summary, players and questions sheets, plus every answer (FR-RP-05).
fn xlsx(r: &SessionReport, snapshot: &QuizSnapshot) -> ApiResult<Vec<u8>> {
    use rust_xlsxwriter::{Format, Workbook, Worksheet};

    let bold = Format::new().set_bold();
    let mut book = Workbook::new();

    fn table(
        sheet: &mut Worksheet,
        bold: &Format,
        headers: &[&str],
        rows: &[Vec<String>],
        numeric: &[usize],
    ) -> Result<(), rust_xlsxwriter::XlsxError> {
        for (c, h) in headers.iter().enumerate() {
            sheet.write_string_with_format(0, c as u16, *h, bold)?;
        }
        for (i, row) in rows.iter().enumerate() {
            let r = i as u32 + 1;
            for (c, cell) in row.iter().enumerate() {
                match cell.parse::<f64>() {
                    Ok(n) if numeric.contains(&c) => sheet.write_number(r, c as u16, n)?,
                    // write_string never makes a formula, so cells are
                    // written as typed; safe_cell is for CSV only.
                    _ => sheet.write_string(r, c as u16, cell)?,
                };
            }
        }
        sheet.set_freeze_panes(1, 0)?;
        sheet.autofit();
        Ok(())
    }

    let s = &r.session;
    let summary: Vec<Vec<String>> = vec![
        vec!["game_name".into(), s.name.clone().unwrap_or_default()],
        vec!["quiz".into(), s.quiz_title.clone()],
        vec!["started_at".into(), s.started_at.clone()],
        vec!["ended_at".into(), s.ended_at.clone().unwrap_or_default()],
        vec!["players".into(), s.player_count.to_string()],
        vec!["questions".into(), r.questions.len().to_string()],
        vec!["average_score".into(), s.average_score.to_string()],
        vec!["participation".into(), r.participation.to_string()],
        vec!["accuracy".into(), r.accuracy.to_string()],
        vec!["average_response_s".into(), seconds(r.average_ms)],
    ];
    table(
        book.add_worksheet().set_name("Summary")?,
        &bold,
        &["item", "value"],
        &summary,
        &[1],
    )?;
    table(
        book.add_worksheet().set_name("Players")?,
        &bold,
        &PLAYER_HEADERS,
        &player_rows(r),
        &[0, 2, 3, 4, 5],
    )?;
    let questions: Vec<Vec<String>> = r
        .questions
        .iter()
        .map(|q| {
            vec![
                (q.index + 1).to_string(),
                q.text.clone(),
                kind_name(q.kind).into(),
                correct_text(snapshot, q.index as usize),
                q.answered.to_string(),
                q.correct.to_string(),
                q.accuracy.to_string(),
                seconds(q.average_ms),
                if q.hard { "yes".into() } else { String::new() },
            ]
        })
        .collect();
    table(
        book.add_worksheet().set_name("Questions")?,
        &bold,
        &[
            "question",
            "text",
            "type",
            "correct_answer",
            "answered",
            "correct",
            "accuracy",
            "average_response_s",
            "hard",
        ],
        &questions,
        &[0, 4, 5, 6, 7],
    )?;
    table(
        book.add_worksheet().set_name("Answers")?,
        &bold,
        &ANSWER_HEADERS,
        &answer_rows(r, snapshot),
        &[0, 2, 8, 9, 10],
    )?;
    Ok(book.save_to_buffer()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_like_cells_are_neutralised() {
        for bad in ["=1+1", "+1", "-1", "@SUM(A1)", "\tx", "\rx", "−1"] {
            assert!(safe_cell(bad).starts_with('\''), "{bad:?}");
        }
        assert_eq!(safe_cell("Hà Nội"), "Hà Nội");
        assert_eq!(safe_cell(""), "");
        assert_eq!(safe_cell("1=1"), "1=1");
    }

    #[test]
    fn csv_has_a_bom_and_sanitised_cells() {
        let bytes = csv_bytes(vec![
            vec!["nickname".into()],
            vec!["=HYPERLINK(\"x\")".into()],
        ])
        .unwrap();
        assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
        assert!(text.contains("'=HYPERLINK"));
    }

    #[test]
    fn file_names_are_ascii() {
        let s = SessionSummary {
            id: "x".into(),
            name: None,
            quiz_title: "Địa lý Việt Nam!".into(),
            started_at: "2026-11-23T09:00:00Z".into(),
            ended_at: None,
            ended: true,
            player_count: 0,
            average_score: 0.0,
        };
        assert_eq!(file_stem(&s), "myquiz-report-dia-ly-viet-nam-20261123");
    }
}
