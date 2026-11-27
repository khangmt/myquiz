//! Runs every script against a real Redis. Skipped unless REDIS_URL is set
//! (CI sets it; locally use infra/docker-compose.dev.yml).

use fred::prelude::*;
use myquiz_engine::answer::{Submission, SubmitError};
use myquiz_engine::quiz::AnswerOption;
use myquiz_engine::{Command, GameState, Question, QuestionKind, QuizSnapshot, TransitionError};

use super::*;

async fn store() -> Option<(LiveStore, Pool)> {
    let url = std::env::var("REDIS_URL").ok()?;
    let pool = Builder::from_config(fred::types::config::Config::from_url(&url).unwrap())
        .build_pool(2)
        .unwrap();
    pool.init().await.unwrap();
    Some((LiveStore::new(pool.clone()), pool))
}

macro_rules! live {
    () => {
        match store().await {
            Some(s) => s,
            None => {
                eprintln!("skipped: REDIS_URL not set");
                return;
            }
        }
    };
}

fn quiz() -> QuizSnapshot {
    let opt = |id, t: &str, c| AnswerOption {
        id,
        text: t.into(),
        is_correct: c,
    };
    let q = |kind, opts| Question {
        kind,
        text: "Q ___".into(),
        media: None,
        markdown: None,
        time_limit_s: 20,
        points: 1000,
        options: opts,
        accepted_answers: vec![],
        ignore_diacritics: false,
    };
    QuizSnapshot {
        title: "Test".into(),
        questions: vec![
            q(
                QuestionKind::Single,
                vec![opt(1, "a", true), opt(2, "b", false)],
            ),
            q(
                QuestionKind::Multi,
                vec![opt(1, "a", true), opt(2, "b", true), opt(3, "c", false)],
            ),
        ],
    }
}

fn player(nick: &str, device: &str) -> NewPlayer {
    NewPlayer {
        nickname: nick.into(),
        avatar: "bunny".into(),
        device_id: device.into(),
        user_id: None,
        token_hash: secrets::hash_token(&secrets::new_player_token()),
    }
}

fn pick(ids: &[u32]) -> Submission {
    Submission {
        option_ids: ids.to_vec(),
        text: None,
    }
}

/// Makes the current question's options visible now (skips the countdown).
async fn skip_countdown(pool: &Pool, pin: &str) {
    let _: () = pool.hset(keys::game(pin), ("startsAt", "0")).await.unwrap();
}

#[tokio::test]
async fn create_join_and_lobby_rules() {
    let (s, _) = live!();
    let pin = s.create_game("sess-1", "host-1", &quiz()).await.unwrap();
    let meta = s.meta(&pin).await.unwrap().unwrap();
    assert_eq!(meta.state, GameState::Lobby);
    assert_eq!(meta.total, 2);
    assert_eq!(s.quiz(&pin).await.unwrap().unwrap(), quiz());

    let an = player("An", "dev-an");
    assert_eq!(s.join(&pin, "p-an", &an).await.unwrap(), Ok(Joined::New(0)));
    assert_eq!(
        s.join(&pin, "p-binh", &player("Bình", "dev-b"))
            .await
            .unwrap(),
        Ok(Joined::New(1))
    );
    s.set_online(&pin, "p-an", 1).await.unwrap();
    assert_eq!(
        s.join(&pin, "p-x", &player("  an ", "dev-x"))
            .await
            .unwrap(),
        Err(JoinRefusal::NickTaken),
        "nicknames are unique ignoring case and spaces"
    );
    s.set_online(&pin, "p-an", -1).await.unwrap();
    assert_eq!(
        s.player_for_token(&pin, &an.token_hash)
            .await
            .unwrap()
            .as_deref(),
        Some("p-an")
    );

    // Mute: kept on the record, and undone.
    assert!(s.set_muted(&pin, "p-binh", true).await.unwrap());
    assert!(s.player(&pin, "p-binh").await.unwrap().unwrap().muted);
    assert!(s.set_muted(&pin, "p-binh", false).await.unwrap());
    assert!(!s.player(&pin, "p-binh").await.unwrap().unwrap().muted);
    assert!(!s.set_muted(&pin, "p-nobody", true).await.unwrap());

    // Remove: gone, token revoked, nickname free again, and the same device
    // may come back under another name.
    assert!(s.kick(&pin, "p-an", false).await.unwrap());
    assert!(!s.kick(&pin, "p-an", false).await.unwrap());
    assert_eq!(
        s.player_for_token(&pin, &an.token_hash).await.unwrap(),
        None
    );
    assert_eq!(
        s.join(&pin, "p-an2", &player("An again", "dev-an"))
            .await
            .unwrap(),
        Ok(Joined::New(2))
    );

    // Ban: that device stays out, under any name.
    assert!(s.kick(&pin, "p-an2", true).await.unwrap());
    assert_eq!(
        s.join(&pin, "p-an3", &player("An thrice", "dev-an"))
            .await
            .unwrap(),
        Err(JoinRefusal::Banned)
    );
    assert_eq!(
        s.join(&pin, "p-chi", &player("An", "dev-chi"))
            .await
            .unwrap(),
        Ok(Joined::New(3))
    );

    s.set_locked(&pin, true).await.unwrap();
    assert_eq!(
        s.join(&pin, "p-d", &player("Dung", "dev-d")).await.unwrap(),
        Err(JoinRefusal::Locked)
    );
    s.set_locked(&pin, false).await.unwrap();

    let players = s.players(&pin).await.unwrap();
    let names: Vec<_> = players.iter().map(|(_, p)| p.nickname.as_str()).collect();
    assert_eq!(names, ["Bình", "An"]);

    assert_eq!(
        s.join("000000", "p", &player("Zed", "d")).await.unwrap(),
        Err(JoinRefusal::PinInvalid)
    );
    s.finish(&pin).await.unwrap();
}

#[tokio::test]
async fn full_game_flow() {
    let (s, pool) = live!();
    let pin = s.create_game("sess-2", "host-2", &quiz()).await.unwrap();
    for (id, nick) in [("p1", "One"), ("p2", "Two"), ("p3", "Three")] {
        s.join(&pin, id, &player(nick, id)).await.unwrap().unwrap();
    }

    // Start: the first question gets a countdown and a deadline.
    let (to, t) = s.command(&pin, Command::Start).await.unwrap().unwrap();
    assert_eq!(to, GameState::Question { index: 0 });
    let Transition::Applied {
        starts_at_ms,
        ends_at_ms,
    } = t
    else {
        panic!("stale")
    };
    assert_eq!(starts_at_ms + 20_000, ends_at_ms);

    // During the countdown answers are refused.
    assert_eq!(
        s.submit_answer(&pin, 0, "p1", &pick(&[1])).await.unwrap(),
        Err(SubmitError::Early)
    );
    skip_countdown(&pool, &pin).await;

    let ok = s
        .submit_answer(&pin, 0, "p1", &pick(&[1]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ok.answered, 1);
    assert_eq!(
        s.submit_answer(&pin, 0, "p1", &pick(&[2])).await.unwrap(),
        Err(SubmitError::Duplicate),
        "first answer wins"
    );
    assert_eq!(
        s.submit_answer(&pin, 1, "p2", &pick(&[1])).await.unwrap(),
        Err(SubmitError::WrongState),
        "wrong question index"
    );
    let ok = s
        .submit_answer(&pin, 0, "p2", &pick(&[2]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ok.answered, 2);

    // Next is refused while a question runs.
    let meta = s.meta(&pin).await.unwrap().unwrap();
    assert_eq!(
        s.command(&pin, Command::Next { expect: meta.state })
            .await
            .unwrap(),
        Err(TransitionError::WrongState)
    );

    // Reveal returns both answers exactly as submitted.
    let mut answers = s.reveal(&pin, 0).await.unwrap().unwrap();
    answers.sort_by(|a, b| a.player.cmp(&b.player));
    assert_eq!(answers.len(), 2);
    assert_eq!(answers[0].player, "p1");
    assert_eq!(answers[0].submission, pick(&[1]));
    assert_eq!(answers[1].submission, pick(&[2]));
    assert!(
        s.reveal(&pin, 0).await.unwrap().is_none(),
        "only one reveal"
    );
    assert_eq!(
        s.submit_answer(&pin, 0, "p3", &pick(&[1])).await.unwrap(),
        Err(SubmitError::WrongState),
        "too late after reveal"
    );

    // Scores apply once.
    let updates = vec![("p1".to_string(), 950, 2_000), ("p2".to_string(), 0, 4_000)];
    assert!(s.apply_scores(&pin, 0, &updates).await.unwrap());
    assert!(!s.apply_scores(&pin, 0, &updates).await.unwrap());
    let p1 = s
        .players(&pin)
        .await
        .unwrap()
        .into_iter()
        .find(|(id, _)| id == "p1")
        .unwrap()
        .1;
    assert_eq!((p1.score, p1.time_ms), (950, 2_000));

    // Reveal → Leaderboard → Question 1; a stale double click is refused.
    let reveal = GameState::Reveal { index: 0 };
    let (to, _) = s
        .command(&pin, Command::Next { expect: reveal })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(to, GameState::Leaderboard { index: 0 });
    assert_eq!(
        s.command(&pin, Command::Next { expect: reveal })
            .await
            .unwrap(),
        Err(TransitionError::Stale)
    );
    let (to, _) = s
        .command(&pin, Command::Next { expect: to })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(to, GameState::Question { index: 1 });

    // Multi-select and empty arrays survive the trip through Lua.
    skip_countdown(&pool, &pin).await;
    s.submit_answer(&pin, 1, "p3", &pick(&[2, 1]))
        .await
        .unwrap()
        .unwrap();
    let answers = s.reveal(&pin, 1).await.unwrap().unwrap();
    assert_eq!(answers[0].submission, pick(&[2, 1]));

    // Finish releases the PIN and schedules expiry.
    assert!(s.finish(&pin).await.unwrap());
    assert!(!s.finish(&pin).await.unwrap());
    let pin_exists: i64 = pool.exists(keys::pin(&pin)).await.unwrap();
    assert_eq!(pin_exists, 0);
    let ttl: i64 = pool.ttl(keys::players(&pin)).await.unwrap();
    assert!(ttl > 0 && ttl <= GAME_TTL_S as i64);
    assert_eq!(s.meta(&pin).await.unwrap().unwrap().state, GameState::Ended);
    assert_eq!(
        s.join(&pin, "late", &player("Late", "late")).await.unwrap(),
        Err(JoinRefusal::PinInvalid)
    );
}

#[tokio::test]
async fn late_answers_are_refused_by_server_time() {
    let (s, pool) = live!();
    let pin = s.create_game("sess-3", "host-3", &quiz()).await.unwrap();
    s.join(&pin, "p1", &player("One", "d1"))
        .await
        .unwrap()
        .unwrap();
    s.command(&pin, Command::Start).await.unwrap().unwrap();
    // Pretend the deadline passed a moment ago.
    let _: () = pool
        .hset(keys::game(&pin), [("startsAt", "0"), ("endsAt", "1")])
        .await
        .unwrap();
    assert_eq!(
        s.submit_answer(&pin, 0, "p1", &pick(&[1])).await.unwrap(),
        Err(SubmitError::Late)
    );
    s.finish(&pin).await.unwrap();
}

#[tokio::test]
async fn deadline_sweep_and_racing_reveals() {
    let (s, pool) = live!();
    let pin = s.create_game("sess-4", "host-4", &quiz()).await.unwrap();
    s.command(&pin, Command::Start).await.unwrap().unwrap();
    assert!(!s.due(10_000).await.unwrap().contains(&pin), "not due yet");

    let _: () = pool
        .zadd(
            keys::DEADLINES,
            None,
            None,
            false,
            false,
            (1.0, pin.as_str()),
        )
        .await
        .unwrap();
    assert!(s.due(10_000).await.unwrap().contains(&pin));

    // Ten workers race to reveal; exactly one wins.
    let wins = futures_join_all((0..10).map(|_| {
        let s = s.clone();
        let pin = pin.clone();
        tokio::spawn(async move { s.reveal(&pin, 0).await.unwrap().is_some() })
    }))
    .await;
    assert_eq!(wins.iter().filter(|w| **w).count(), 1);
    assert!(
        !s.due(10_000).await.unwrap().contains(&pin),
        "removed from the sweep"
    );
    s.finish(&pin).await.unwrap();
}

async fn futures_join_all<I>(handles: I) -> Vec<bool>
where
    I: IntoIterator<Item = tokio::task::JoinHandle<bool>>,
{
    let mut out = Vec::new();
    for h in handles {
        out.push(h.await.unwrap());
    }
    out
}

#[tokio::test]
async fn pins_are_unique_among_active_games() {
    let (s, _) = live!();
    let mut pins = std::collections::HashSet::new();
    for i in 0..50 {
        let pin = s
            .create_game(&format!("sess-u{i}"), "h", &quiz())
            .await
            .unwrap();
        assert!(pins.insert(pin));
    }
    for pin in &pins {
        s.finish(pin).await.unwrap();
    }
}

#[tokio::test]
async fn returning_players_take_their_name_back() {
    let (s, _) = live!();
    let pin = s
        .create_game("sess-rejoin", "host-r", &quiz())
        .await
        .unwrap();
    let first = player("Khánh", "dev-1");
    assert_eq!(
        s.join(&pin, "p-k", &first).await.unwrap(),
        Ok(Joined::New(0))
    );

    // Online: the name is taken, whoever asks.
    s.set_online(&pin, "p-k", 1).await.unwrap();
    assert_eq!(
        s.join(&pin, "x", &player("khánh", "dev-1")).await.unwrap(),
        Err(JoinRefusal::NickTaken)
    );
    s.set_online(&pin, "p-k", -1).await.unwrap();

    // Offline, same device (storage lost): taken back, even when locked,
    // with a new token and the old one revoked.
    s.set_locked(&pin, true).await.unwrap();
    let again = player("Khánh", "dev-1");
    assert_eq!(
        s.join(&pin, "x", &again).await.unwrap(),
        Ok(Joined::Reclaimed {
            player_id: "p-k".into()
        })
    );
    assert_eq!(
        s.player_for_token(&pin, &again.token_hash)
            .await
            .unwrap()
            .as_deref(),
        Some("p-k")
    );
    assert_eq!(
        s.player_for_token(&pin, &first.token_hash).await.unwrap(),
        None
    );

    // Another device must wait for the host.
    let other = player("Khánh", "dev-2");
    assert_eq!(
        s.join(&pin, "y", &other).await.unwrap(),
        Err(JoinRefusal::RejoinWait {
            player_id: "p-k".into()
        })
    );
    assert!(!s.allow_rejoin(&pin, "nobody").await.unwrap());
    assert!(s.allow_rejoin(&pin, "p-k").await.unwrap());
    // A third device is not let in by that approval.
    assert!(matches!(
        s.join(&pin, "z", &player("Khánh", "dev-3")).await.unwrap(),
        Err(JoinRefusal::RejoinWait { .. })
    ));
    assert_eq!(
        s.join(&pin, "y", &player("Khánh", "dev-2")).await.unwrap(),
        Ok(Joined::Reclaimed {
            player_id: "p-k".into()
        })
    );
    // The record follows the new device; still one player.
    let players = s.players(&pin).await.unwrap();
    assert_eq!(players.len(), 1);
    assert_eq!(players[0].1.device_id, "dev-2");

    // New names are still refused while locked.
    assert_eq!(
        s.join(&pin, "n", &player("New", "dev-9")).await.unwrap(),
        Err(JoinRefusal::Locked)
    );
    s.finish(&pin).await.unwrap();
}

#[tokio::test]
async fn answers_get_a_grace_period_and_reveal_waits_for_the_room() {
    let (s, pool) = live!();
    let pin = s
        .create_game("sess-grace", "host-g", &quiz())
        .await
        .unwrap();
    for id in ["a", "b", "c"] {
        s.join(&pin, id, &player(id, id)).await.unwrap().unwrap();
        s.set_online(&pin, id, 1).await.unwrap();
    }
    s.command(&pin, Command::Start).await.unwrap().unwrap();
    // The sweep is scheduled past the deadline by the grace period.
    let meta = s.meta(&pin).await.unwrap().unwrap();
    let due: f64 = pool.zscore(keys::DEADLINES, &pin).await.unwrap();
    assert_eq!(due as u64, meta.ends_at_ms + ANSWER_GRACE_MS);

    // A moment after the deadline still counts, charged the full time.
    let now = s.now_ms().await.unwrap();
    let _: () = pool
        .hset(
            keys::game(&pin),
            [
                ("startsAt", (now - 20_100).to_string()),
                ("endsAt", (now - 100).to_string()),
            ],
        )
        .await
        .unwrap();
    let ok = s
        .submit_answer(&pin, 0, "a", &pick(&[1]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ok.response_ms, 20_000);

    // "b" answers and drops off: "c" is still in the room and has not.
    s.submit_answer(&pin, 0, "b", &pick(&[1]))
        .await
        .unwrap()
        .unwrap();
    s.set_online(&pin, "b", -1).await.unwrap();
    assert!(!s.all_connected_answered(&pin, 0).await.unwrap());
    s.submit_answer(&pin, 0, "c", &pick(&[2]))
        .await
        .unwrap()
        .unwrap();
    assert!(s.all_connected_answered(&pin, 0).await.unwrap());

    // Past the grace period: late.
    let now = s.now_ms().await.unwrap();
    let _: () = pool
        .hset(
            keys::game(&pin),
            ("endsAt", (now - ANSWER_GRACE_MS - 10).to_string()),
        )
        .await
        .unwrap();
    s.join(&pin, "d", &player("d", "d")).await.unwrap().unwrap();
    assert_eq!(
        s.submit_answer(&pin, 0, "d", &pick(&[1])).await.unwrap(),
        Err(SubmitError::Late)
    );
    s.finish(&pin).await.unwrap();
}
