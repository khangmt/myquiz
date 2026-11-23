//! The daily clean-up (design section 8; FR-QZ-11, FR-QZ-18 to 20,
//! NFR-18, NFR-22, SEC-35).
//!
//! Once a day, at CLEANUP_HOUR Vietnam time, whichever api instance takes a
//! PostgreSQL advisory lock:
//!
//! 1. emails each owner about quizzes due for deletion in 7 days and in 1
//!    day, recording each email in `retention_notices` so none goes twice;
//! 2. deletes quizzes past their date (`last_used_at + RETENTION_DAYS`)
//!    that are not in a live game, and quizzes soft-deleted more than 30
//!    days ago, 100 at a time, with images no other quiz uses and one
//!    audit entry each;
//! 3. deletes game reports older than 12 months.
//!
//! Every step is safe to run again. `myquiz cleanup` runs it once by hand.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Context;
use lettre::message::{header::ContentType, Mailbox};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde_json::json;
use sqlx::PgPool;
use time::{OffsetDateTime, UtcOffset};
use uuid::Uuid;

use crate::config::Config;

/// Any constant both replicas agree on.
const LOCK_KEY: i64 = 0x6d79_7175_697a_0001;
const BATCH: i64 = 100;
const SOFT_DELETE_DAYS: i32 = 30;
const REPORT_MONTHS: i32 = 12;
/// Lobbies or games this old are treated as abandoned, so a crashed game
/// never keeps its quiz forever.
const LIVE_GAME_HOURS: i32 = 24;
const ICT: UtcOffset = match UtcOffset::from_hms(7, 0, 0) {
    Ok(o) => o,
    Err(_) => panic!("valid offset"),
};

/// An email as sent, for logs and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub to: String,
    pub subject: String,
    pub body: String,
}

#[derive(Clone)]
pub enum Mailer {
    Smtp {
        transport: Box<AsyncSmtpTransport<Tokio1Executor>>,
        from: Mailbox,
    },
    /// No SMTP_URL: nothing is sent and nothing recorded, so the emails go
    /// out once a mail server is configured.
    Disabled,
    #[cfg(test)]
    Memory(Arc<Mutex<Vec<Sent>>>),
}

impl Mailer {
    pub fn from_config(cfg: &Config) -> anyhow::Result<Self> {
        match cfg
            .cleanup
            .smtp_url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
        {
            None => Ok(Mailer::Disabled),
            Some(url) => Ok(Mailer::Smtp {
                transport: Box::new(
                    AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
                        .context("SMTP_URL is not a valid smtp:// or smtps:// URL")?
                        .timeout(Some(Duration::from_secs(20)))
                        .build(),
                ),
                from: cfg
                    .cleanup
                    .mail_from
                    .parse()
                    .context("MAIL_FROM is not a valid address")?,
            }),
        }
    }

    /// True if the email went out.
    async fn send(&self, mail: Sent) -> anyhow::Result<bool> {
        match self {
            Mailer::Disabled => Ok(false),
            Mailer::Smtp { transport, from } => {
                let msg = Message::builder()
                    .from(from.clone())
                    .to(mail.to.parse()?)
                    .subject(mail.subject)
                    .header(ContentType::TEXT_PLAIN)
                    .body(mail.body)?;
                transport.send(msg).await?;
                Ok(true)
            }
            #[cfg(test)]
            Mailer::Memory(sent) => {
                sent.lock().unwrap().push(mail);
                Ok(true)
            }
        }
    }
}

pub struct Cleanup {
    pub db: PgPool,
    pub media_dir: PathBuf,
    pub retention_days: u32,
    pub public_url: String,
    pub mailer: Mailer,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub emails: u32,
    pub quizzes_deleted: u32,
    pub files_removed: u32,
    pub reports_deleted: u64,
}

impl Cleanup {
    pub fn from_config(db: PgPool, cfg: &Config) -> anyhow::Result<Self> {
        Ok(Cleanup {
            db,
            media_dir: PathBuf::from(&cfg.media_dir),
            retention_days: cfg.retention_days,
            public_url: cfg.auth.public_url.trim_end_matches('/').to_string(),
            mailer: Mailer::from_config(cfg)?,
        })
    }

    /// Runs every step if this instance gets the lock; `None` if another
    /// instance is running it.
    pub async fn run_locked(&self) -> anyhow::Result<Option<Outcome>> {
        let mut conn = self.db.acquire().await?;
        let got: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(LOCK_KEY)
            .fetch_one(&mut *conn)
            .await?;
        if !got {
            return Ok(None);
        }
        let result = self.run().await;
        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(LOCK_KEY)
            .execute(&mut *conn)
            .await?;
        result.map(Some)
    }

    pub async fn run(&self) -> anyhow::Result<Outcome> {
        let mut out = Outcome {
            emails: self.send_notices().await?,
            ..Outcome::default()
        };
        let (quizzes, files) = self.delete_quizzes().await?;
        out.quizzes_deleted = quizzes;
        out.files_removed = files;
        out.reports_deleted = self.delete_old_reports().await?;
        tracing::info!(
            emails = out.emails,
            quizzes_deleted = out.quizzes_deleted,
            files_removed = out.files_removed,
            reports_deleted = out.reports_deleted,
            "clean-up finished"
        );
        Ok(out)
    }

    /// FR-QZ-20: one email per owner and kind, listing that owner's quizzes.
    async fn send_notices(&self) -> anyhow::Result<u32> {
        let mut sent = 0;
        if matches!(self.mailer, Mailer::Disabled) {
            tracing::info!("SMTP_URL not set: no reminder emails sent");
            return Ok(0);
        }
        for (kind, days) in [("7d", 7), ("1d", 1)] {
            #[allow(clippy::type_complexity)]
            let rows: Vec<(Uuid, String, Uuid, String, String, String, OffsetDateTime)> =
                sqlx::query_as(
                    "SELECT q.id, q.title, u.id, u.email, u.locale, u.display_name, \
                            q.last_used_at + make_interval(days => $1) AS expires \
                     FROM quizzes q JOIN users u ON u.id = q.owner_id \
                     WHERE q.deleted_at IS NULL AND u.deleted_at IS NULL AND u.status = 'active' \
                       AND q.last_used_at + make_interval(days => $1) > now() \
                       AND q.last_used_at + make_interval(days => $1) <= now() + make_interval(days => $2) \
                       AND ($3 = '1d' OR q.last_used_at + make_interval(days => $1) > now() + interval '1 day') \
                       AND NOT EXISTS (SELECT 1 FROM retention_notices n \
                                       WHERE n.quiz_id = q.id AND n.kind = $3 AND n.sent_at > q.last_used_at) \
                     ORDER BY u.id, expires, q.title",
                )
                .bind(self.retention_days as i32)
                .bind(days)
                .bind(kind)
                .fetch_all(&self.db)
                .await?;
            let mut by_user: BTreeMap<Uuid, Vec<_>> = BTreeMap::new();
            for r in rows {
                by_user.entry(r.2).or_default().push(r);
            }
            for (user, quizzes) in by_user {
                let (_, _, _, email, locale, name, _) = &quizzes[0];
                let list: Vec<(String, OffsetDateTime)> =
                    quizzes.iter().map(|q| (q.1.clone(), q.6)).collect();
                let mail = reminder(email, name, locale, kind, &list, &self.public_url);
                match self.mailer.send(mail).await {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(e) => {
                        // One bad address must not stop the rest.
                        tracing::warn!(error = %e, %user, "reminder email failed");
                        continue;
                    }
                }
                sent += 1;
                let ids: Vec<Uuid> = quizzes.iter().map(|q| q.0).collect();
                let mut tx = self.db.begin().await?;
                sqlx::query(
                    "INSERT INTO retention_notices (user_id, quiz_id, kind) \
                     SELECT $1, unnest($2::uuid[]), $3 \
                     ON CONFLICT (quiz_id, kind) DO UPDATE SET sent_at = now()",
                )
                .bind(user)
                .bind(&ids)
                .bind(kind)
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    "INSERT INTO audit_log (actor_user_id, action, target_type, target_id, details) \
                     VALUES (NULL, 'retention_notice_sent', 'user', $1, $2)",
                )
                .bind(user.to_string())
                .bind(json!({ "kind": kind, "quizIds": ids }))
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
            }
        }
        Ok(sent)
    }

    /// FR-QZ-18, FR-QZ-11: deletes in batches until nothing is due.
    async fn delete_quizzes(&self) -> anyhow::Result<(u32, u32)> {
        let (mut quizzes, mut files) = (0, 0);
        loop {
            let mut tx = self.db.begin().await?;
            let due: Vec<(Uuid, String, Uuid, bool)> = sqlx::query_as(
                "SELECT q.id, q.title, q.owner_id, q.deleted_at IS NOT NULL FROM quizzes q \
                 WHERE ((q.deleted_at IS NULL AND q.last_used_at + make_interval(days => $1) <= now()) \
                        OR q.deleted_at <= now() - make_interval(days => $2)) \
                   AND NOT EXISTS (SELECT 1 FROM game_sessions s WHERE s.quiz_id = q.id \
                                   AND s.status <> 'ended' \
                                   AND s.created_at > now() - make_interval(hours => $3)) \
                 ORDER BY q.id LIMIT $4 FOR UPDATE SKIP LOCKED",
            )
            .bind(self.retention_days as i32)
            .bind(SOFT_DELETE_DAYS)
            .bind(LIVE_GAME_HOURS)
            .bind(BATCH)
            .fetch_all(&mut *tx)
            .await?;
            if due.is_empty() {
                break;
            }
            let ids: Vec<Uuid> = due.iter().map(|d| d.0).collect();
            let media: Vec<Uuid> = sqlx::query_scalar(
                "SELECT cover_media_id FROM quizzes WHERE id = ANY($1) AND cover_media_id IS NOT NULL \
                 UNION SELECT media_id FROM questions WHERE quiz_id = ANY($1) AND media_id IS NOT NULL",
            )
            .bind(&ids)
            .fetch_all(&mut *tx)
            .await?;
            sqlx::query("DELETE FROM quizzes WHERE id = ANY($1)")
                .bind(&ids)
                .execute(&mut *tx)
                .await?;
            let keys: Vec<String> = sqlx::query_scalar(
                "DELETE FROM media m WHERE m.id = ANY($1) \
                   AND NOT EXISTS (SELECT 1 FROM quizzes WHERE cover_media_id = m.id) \
                   AND NOT EXISTS (SELECT 1 FROM questions WHERE media_id = m.id) \
                 RETURNING storage_key",
            )
            .bind(&media)
            .fetch_all(&mut *tx)
            .await?;
            for (id, title, owner, soft) in &due {
                sqlx::query(
                    "INSERT INTO audit_log (actor_user_id, action, target_type, target_id, details) \
                     VALUES (NULL, 'quiz_deleted_retention', 'quiz', $1, $2)",
                )
                .bind(id.to_string())
                .bind(json!({
                    "title": title,
                    "ownerId": owner,
                    "reason": if *soft { "deleted_by_owner_30_days" } else { "unused_for_retention_days" },
                    "retentionDays": self.retention_days,
                }))
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
            quizzes += due.len() as u32;
            // Files go after the commit: a failed commit leaves them in
            // place, and a missing file is not an error.
            for key in keys {
                if remove_media(&self.media_dir, &key).await {
                    files += 1;
                }
            }
        }
        Ok((quizzes, files))
    }

    /// NFR-18: reports are kept 12 months.
    async fn delete_old_reports(&self) -> anyhow::Result<u64> {
        let mut total = 0;
        loop {
            let n = sqlx::query(
                "DELETE FROM game_sessions WHERE id IN ( \
                   SELECT id FROM game_sessions \
                   WHERE created_at < now() - make_interval(months => $1) LIMIT 500)",
            )
            .bind(REPORT_MONTHS)
            .execute(&self.db)
            .await?
            .rows_affected();
            total += n;
            if n == 0 {
                return Ok(total);
            }
        }
    }
}

async fn remove_media(dir: &Path, key: &str) -> bool {
    if !crate::media::is_storage_key(key) {
        return false;
    }
    match tokio::fs::remove_file(dir.join(key)).await {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => {
            tracing::warn!(error = %e, key, "could not remove media file");
            false
        }
    }
}

fn date(t: OffsetDateTime, vi: bool) -> String {
    let t = t.to_offset(ICT);
    if vi {
        format!("{:02}/{:02}/{}", t.day(), u8::from(t.month()), t.year())
    } else {
        format!("{} {} {}", t.day(), t.month(), t.year())
    }
}

/// The reminder text, in the owner's language.
fn reminder(
    to: &str,
    name: &str,
    locale: &str,
    kind: &str,
    quizzes: &[(String, OffsetDateTime)],
    public_url: &str,
) -> Sent {
    let vi = locale != "en";
    let list: String = quizzes
        .iter()
        .map(|(title, at)| format!("  - {title} ({})\n", date(*at, vi)))
        .collect();
    let link = format!("{public_url}/quizzes");
    let (subject, body) = if vi {
        let when = if kind == "1d" {
            "ngày mai"
        } else {
            "sau 7 ngày"
        };
        (
            format!("myquiz: {} bộ câu hỏi sẽ bị xoá {when}", quizzes.len()),
            format!(
                "Chào {name},\n\n\
                 Các bộ câu hỏi sau chưa được sửa hoặc dùng để chơi trong 3 tuần nên sẽ bị xoá {when}:\n\n\
                 {list}\n\
                 Để giữ một bộ câu hỏi, hãy mở và sửa nó, hoặc tổ chức một trò chơi với nó; hạn xoá sẽ được tính lại từ đầu. \
                 Bạn cũng có thể tải tệp zip để lưu trữ và tải lên lại bất cứ lúc nào.\n\n\
                 {link}\n\n\
                 Báo cáo của các trò chơi đã chơi không bị ảnh hưởng.\n"
            ),
        )
    } else {
        let when = if kind == "1d" {
            "tomorrow"
        } else {
            "in 7 days"
        };
        (
            format!("myquiz: {} quiz(zes) will be deleted {when}", quizzes.len()),
            format!(
                "Hello {name},\n\n\
                 These quizzes have not been edited or played for 3 weeks, so they will be deleted {when}:\n\n\
                 {list}\n\
                 To keep one, open and edit it, or host a game with it; that restarts its clock. \
                 You can also download it as a zip and upload it again at any time.\n\n\
                 {link}\n\n\
                 Reports of games already played are not affected.\n"
            ),
        )
    };
    Sent {
        to: to.to_string(),
        subject,
        body,
    }
}

/// When the next run is due after `now`: today or tomorrow at `hour`
/// Vietnam time.
pub fn next_run(now: OffsetDateTime, hour: u8) -> OffsetDateTime {
    let local = now.to_offset(ICT);
    let today = local.replace_time(time::Time::from_hms(hour.min(23), 0, 0).expect("valid hour"));
    if today > local {
        today
    } else {
        today + time::Duration::days(1)
    }
}

/// Runs the clean-up every day at CLEANUP_HOUR in the background.
pub fn spawn_daily(cleanup: Cleanup, hour: u8) {
    let cleanup = Arc::new(cleanup);
    tokio::spawn(async move {
        loop {
            let now = OffsetDateTime::now_utc();
            let wait = (next_run(now, hour) - now).unsigned_abs();
            tracing::info!(in_s = wait.as_secs(), "next clean-up scheduled");
            tokio::time::sleep(wait).await;
            match cleanup.run_locked().await {
                Ok(Some(_)) => {}
                Ok(None) => tracing::info!("clean-up already running on another instance"),
                Err(e) => tracing::error!(error = %e, "clean-up failed"),
            }
            // Never run twice in the same minute.
            tokio::time::sleep(Duration::from_secs(61)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn next_run_is_the_coming_two_am_in_vietnam() {
        // 18:00 UTC = 01:00 ICT: later the same ICT day.
        assert_eq!(
            next_run(datetime!(2026-12-07 18:00 UTC), 2),
            datetime!(2026-12-08 02:00 +7)
        );
        // 20:00 UTC = 03:00 ICT: the next day.
        assert_eq!(
            next_run(datetime!(2026-12-07 20:00 UTC), 2),
            datetime!(2026-12-09 02:00 +7)
        );
    }

    #[test]
    fn reminders_are_in_the_owners_language() {
        let at = datetime!(2026-12-08 03:00 UTC);
        let vi = reminder(
            "a@x",
            "Lan",
            "vi",
            "1d",
            &[("Toán".into(), at)],
            "https://q",
        );
        assert!(vi.subject.contains("ngày mai"));
        assert!(vi.body.contains("Toán (08/12/2026)"));
        assert!(vi.body.contains("https://q/quizzes"));
        let en = reminder(
            "a@x",
            "Lan",
            "en",
            "7d",
            &[("Maths".into(), at)],
            "https://q",
        );
        assert!(en.subject.contains("in 7 days"));
        assert!(en.body.contains("Maths (8 December 2026)"));
    }
}

#[cfg(test)]
mod db_tests {
    //! Against a real PostgreSQL; skipped unless DATABASE_URL is set.
    use super::*;

    async fn db() -> Option<PgPool> {
        let url = std::env::var("DATABASE_URL").ok()?;
        let db = PgPool::connect(&url).await.ok()?;
        crate::MIGRATOR.run(&db).await.unwrap();
        Some(db)
    }

    async fn user(db: &PgPool, locale: &str) -> Uuid {
        sqlx::query_scalar(
            "INSERT INTO users (email, display_name, locale) VALUES ($1, 'Lan', $2) RETURNING id",
        )
        .bind(format!("{}@uit.edu.vn", Uuid::new_v4()))
        .bind(locale)
        .fetch_one(db)
        .await
        .unwrap()
    }

    /// A quiz last used `hours_ago` hours ago, with one image of its own.
    async fn quiz(
        db: &PgPool,
        owner: Uuid,
        title: &str,
        hours_ago: i32,
        dir: &Path,
    ) -> (Uuid, String) {
        let key = format!("{}.webp", Uuid::new_v4().simple());
        std::fs::write(dir.join(&key), b"img").unwrap();
        let media: Uuid = sqlx::query_scalar(
            "INSERT INTO media (owner_id, storage_key, mime, bytes, width, height) \
             VALUES ($1, $2, 'image/webp', 3, 1, 1) RETURNING id",
        )
        .bind(owner)
        .bind(&key)
        .fetch_one(db)
        .await
        .unwrap();
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO quizzes (owner_id, title, cover_media_id, last_used_at) \
             VALUES ($1, $2, $3, now() - make_interval(hours => $4)) RETURNING id",
        )
        .bind(owner)
        .bind(title)
        .bind(media)
        .bind(hours_ago)
        .fetch_one(db)
        .await
        .unwrap();
        (id, key)
    }

    async fn exists(db: &PgPool, id: Uuid) -> bool {
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM quizzes WHERE id = $1)")
            .bind(id)
            .fetch_one(db)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn reminders_then_deletion_then_nothing_twice() {
        let Some(db) = db().await else {
            eprintln!("skipped: DATABASE_URL not set");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let owner = user(&db, "vi").await;
        let fresh = quiz(&db, owner, "Mới", 24, dir.path()).await;
        let week = quiz(&db, owner, "Còn 6 ngày", 15 * 24, dir.path()).await;
        let day = quiz(&db, owner, "Còn nửa ngày", 20 * 24 + 12, dir.path()).await;
        let due = quiz(&db, owner, "Hết hạn", 22 * 24, dir.path()).await;
        // A quiz in a live game is kept even past its date.
        let playing = quiz(&db, owner, "Đang chơi", 30 * 24, dir.path()).await;
        sqlx::query(
            "INSERT INTO game_sessions (quiz_id, host_id, quiz_snapshot, status) \
             VALUES ($1, $2, '{}', 'running')",
        )
        .bind(playing.0)
        .bind(owner)
        .execute(&db)
        .await
        .unwrap();
        // Soft-deleted 31 days ago: purged whatever its last use.
        let trashed = quiz(&db, owner, "Thùng rác", 0, dir.path()).await;
        sqlx::query("UPDATE quizzes SET deleted_at = now() - interval '31 days' WHERE id = $1")
            .bind(trashed.0)
            .execute(&db)
            .await
            .unwrap();
        // A report older than 12 months goes; a recent one stays.
        let old_report: Uuid = sqlx::query_scalar(
            "INSERT INTO game_sessions (host_id, quiz_snapshot, status, created_at) \
             VALUES ($1, '{}', 'ended', now() - interval '13 months') RETURNING id",
        )
        .bind(owner)
        .fetch_one(&db)
        .await
        .unwrap();

        let sent = Arc::new(Mutex::new(Vec::new()));
        let job = Cleanup {
            db: db.clone(),
            media_dir: dir.path().to_path_buf(),
            retention_days: 21,
            public_url: "https://quiz.example".into(),
            mailer: Mailer::Memory(sent.clone()),
        };
        let out = job.run_locked().await.unwrap().expect("got the lock");

        // Other tests share the database, so look only at this owner.
        let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
            .bind(owner)
            .fetch_one(&db)
            .await
            .unwrap();
        let mine: Vec<Sent> = sent
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.to == email)
            .cloned()
            .collect();
        assert_eq!(mine.len(), 2, "{mine:?}");
        let week_mail = mine.iter().find(|m| m.subject.contains("7 ngày")).unwrap();
        assert!(week_mail.body.contains("Còn 6 ngày"));
        assert!(!week_mail.body.contains("Còn nửa ngày"));
        let day_mail = mine
            .iter()
            .find(|m| m.subject.contains("ngày mai"))
            .unwrap();
        assert!(day_mail.body.contains("Còn nửa ngày"));
        assert!(!day_mail.body.contains("Mới"));

        assert!(
            exists(&db, fresh.0).await && exists(&db, week.0).await && exists(&db, day.0).await
        );
        assert!(!exists(&db, due.0).await);
        assert!(exists(&db, playing.0).await);
        assert!(!exists(&db, trashed.0).await);
        assert!(!dir.path().join(&due.1).exists());
        assert!(!dir.path().join(&trashed.1).exists());
        assert!(dir.path().join(&fresh.1).exists());
        assert!(out.quizzes_deleted >= 2 && out.files_removed >= 2);
        let audits: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM audit_log WHERE action = 'quiz_deleted_retention' \
             AND target_id = ANY($1)",
        )
        .bind(vec![due.0.to_string(), trashed.0.to_string()])
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(audits, 2);
        let report_left: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM game_sessions WHERE id = $1)")
                .bind(old_report)
                .fetch_one(&db)
                .await
                .unwrap();
        assert!(!report_left);

        // A second run sends nothing new to this owner.
        let before = sent.lock().unwrap().len();
        job.run_locked().await.unwrap();
        let again = sent.lock().unwrap()[before..]
            .iter()
            .filter(|m| m.to == mine[0].to)
            .count();
        assert_eq!(again, 0);

        // Using the quiz restarts its clock, so a later reminder may go.
        sqlx::query("UPDATE quizzes SET last_used_at = now() - interval '15 days' WHERE id = $1")
            .bind(day.0)
            .execute(&db)
            .await
            .unwrap();
        let before = sent.lock().unwrap().len();
        job.run_locked().await.unwrap();
        let later: Vec<Sent> = sent.lock().unwrap()[before..]
            .iter()
            .filter(|m| m.to == mine[0].to)
            .cloned()
            .collect();
        assert_eq!(later.len(), 1);
        assert!(later[0].body.contains("Còn nửa ngày"));
    }
}
