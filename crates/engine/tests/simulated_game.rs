//! A whole game driven only through the engine's public API, the way the
//! WebSocket gateway will use it: state transitions, answer checks, scoring
//! at reveal, totals and ranking.

use std::collections::HashMap;

use myquiz_engine::answer::{check_timing, response_ms, validate_submission};
use myquiz_engine::quiz::AnswerOption;
use myquiz_engine::ranking::{leaderboard, podium, rank, Standing};
use myquiz_engine::reveal::{score_question, time_charged_ms, StoredAnswer};
use myquiz_engine::view::PublicQuestion;
use myquiz_engine::*;

fn quiz() -> QuizSnapshot {
    let opt = |id, text: &str, is_correct| AnswerOption {
        id,
        text: text.into(),
        is_correct,
    };
    QuizSnapshot {
        title: "Lịch sử".into(),
        questions: vec![
            Question {
                kind: QuestionKind::Single,
                text: "Năm nào?".into(),
                media: None,
                time_limit_s: 20,
                points: 1000,
                options: vec![opt(1, "1945", true), opt(2, "1954", false)],
                accepted_answers: vec![],
                ignore_diacritics: false,
            },
            Question {
                kind: QuestionKind::FillBlank,
                text: "Thủ đô là ___.".into(),
                media: None,
                time_limit_s: 30,
                points: 2000,
                options: vec![],
                accepted_answers: vec!["Hà Nội".into()],
                ignore_diacritics: true,
            },
        ],
    }
}

struct Player {
    join_order: u32,
    score: u64,
    time_ms: u64,
}

#[test]
fn three_players_two_questions() {
    let quiz = quiz();
    assert!(quiz.validate().is_empty());
    let total = quiz.total();

    let mut players: HashMap<&str, Player> = ["an", "binh", "chi"]
        .iter()
        .enumerate()
        .map(|(i, n)| {
            (
                *n,
                Player {
                    join_order: i as u32,
                    score: 0,
                    time_ms: 0,
                },
            )
        })
        .collect();

    // What each player does per question: (player, submission, ms after start).
    let script: [Vec<(&str, Submission, u64)>; 2] = [
        vec![
            (
                "an",
                Submission {
                    option_ids: vec![1],
                    text: None,
                },
                2_000,
            ),
            (
                "binh",
                Submission {
                    option_ids: vec![1],
                    text: None,
                },
                6_000,
            ),
            (
                "chi",
                Submission {
                    option_ids: vec![2],
                    text: None,
                },
                1_000,
            ),
        ],
        vec![
            (
                "an",
                Submission {
                    option_ids: vec![],
                    text: Some("ha noi".into()),
                },
                15_000,
            ),
            (
                "binh",
                Submission {
                    option_ids: vec![],
                    text: Some("Hà Nội".into()),
                },
                3_000,
            ),
            // chi does not answer
        ],
    ];

    let mut state = GameState::Lobby.apply(Command::Start, total).unwrap();
    let mut clock = 1_000_000u64;

    while let GameState::Question { index } = state {
        let q = quiz.question(index).unwrap();
        let shown = PublicQuestion::from_question(index, total, q);
        assert_eq!(shown.index, index);

        let starts_at = clock + 3_000; // "get ready" countdown
        let ends_at = starts_at + q.time_limit_ms();

        // An eager client cannot answer during the countdown.
        assert_eq!(
            check_timing(&state, index, clock, starts_at, ends_at),
            Err(SubmitError::Early)
        );

        let mut stored = Vec::new();
        for (who, sub, after) in &script[index] {
            let now = starts_at + after;
            validate_submission(q, sub).unwrap();
            check_timing(&state, index, now, starts_at, ends_at).unwrap();
            stored.push(StoredAnswer {
                player: *who,
                submission: sub.clone(),
                response_ms: response_ms(starts_at, now),
            });
        }
        // Too late for anyone after the deadline.
        assert_eq!(
            check_timing(&state, index, ends_at, starts_at, ends_at),
            Err(SubmitError::Late)
        );

        state = state.apply(Command::Deadline { index }, total).unwrap();
        let outcome = score_question(q, &stored);
        for (name, p) in players.iter_mut() {
            let result = outcome.results.iter().find(|r| r.player == *name);
            p.score += u64::from(result.map_or(0, |r| r.points));
            p.time_ms += time_charged_ms(q, result.map(|r| r.response_ms));
        }

        state = state.apply(Command::Next { expect: state }, total).unwrap(); // leaderboard
        state = state.apply(Command::Next { expect: state }, total).unwrap(); // next question or podium
        clock = ends_at + 10_000;
    }
    assert_eq!(state, GameState::Podium);

    let ranked = rank(
        players
            .iter()
            .map(|(name, p)| Standing {
                player: *name,
                score: p.score,
                total_time_ms: p.time_ms,
                join_order: p.join_order,
            })
            .collect(),
    );
    // Q1 (20 s, 1000): an 2 s → 950, binh 6 s → 850, chi wrong → 0.
    // Q2 (30 s, 2000): an 15 s → 1500, binh 3 s → 1900, chi silent → 0.
    let table: Vec<_> = ranked
        .iter()
        .map(|r| (r.rank, r.standing.player, r.standing.score))
        .collect();
    assert_eq!(table, [(1, "binh", 2750), (2, "an", 2450), (3, "chi", 0)]);
    assert_eq!(podium(&ranked).len(), 3);
    assert_eq!(leaderboard(&ranked).len(), 3);

    assert_eq!(
        state.apply(Command::Next { expect: state }, total),
        Ok(GameState::Ended)
    );
}
