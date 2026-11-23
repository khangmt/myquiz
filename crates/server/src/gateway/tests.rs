//! End-to-end games over real WebSockets with scripted bots, against a real
//! PostgreSQL and Redis. Skipped unless DATABASE_URL and REDIS_URL are set.

use std::collections::VecDeque;
use std::time::Duration;

use fred::prelude::*;
use futures_util::{SinkExt, StreamExt};
use myquiz_engine::quiz::AnswerOption;
use myquiz_engine::{Question, QuestionKind, QuizSnapshot};
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

use super::game::GameService;
use super::hub::Hub;
use crate::live::{keys, LiveStore};

const WAIT: Duration = Duration::from_secs(15);

struct Env {
    db: PgPool,
    redis: Pool,
    store: LiveStore,
    addr: String,
}

async fn env() -> Option<Env> {
    let db_url = std::env::var("DATABASE_URL").ok()?;
    let redis_url = std::env::var("REDIS_URL").ok()?;
    let db = PgPool::connect(&db_url).await.unwrap();
    crate::MIGRATOR.run(&db).await.unwrap();
    let redis = Builder::from_config(fred::types::config::Config::from_url(&redis_url).unwrap())
        .build_pool(2)
        .unwrap();
    redis.init().await.unwrap();

    // A short countdown keeps the test fast; the deadline path still waits
    // for a real 5 s question.
    let store = LiveStore::with_countdown(redis.clone(), 50);
    let hub = Hub::start(redis.clone(), &redis_url).await.unwrap();
    let service = GameService::new(store.clone(), db.clone(), hub);
    super::spawn_sweep(service.clone());
    super::spawn_suspended_sweep(service.clone());
    let app = super::router(service, super::AllowedOrigins::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Some(Env {
        db,
        redis,
        store,
        addr,
    })
}

macro_rules! env_or_skip {
    () => {
        match env().await {
            Some(e) => e,
            None => {
                eprintln!("skipped: DATABASE_URL and REDIS_URL not set");
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
        text: "Q?".into(),
        media: None,
        time_limit_s: 5,
        points: 1000,
        options: opts,
        accepted_answers: vec![],
        ignore_diacritics: false,
    };
    QuizSnapshot {
        title: "Bots".into(),
        questions: vec![
            q(
                QuestionKind::Single,
                vec![opt(1, "right", true), opt(2, "wrong", false)],
            ),
            q(
                QuestionKind::Multi,
                vec![opt(1, "a", true), opt(2, "b", true), opt(3, "c", false)],
            ),
        ],
    }
}

/// Seeds a host account, a login session and a game; returns (pin, sid, session id).
async fn seed_game(env: &Env) -> (String, String, Uuid) {
    let host = Uuid::new_v4();
    sqlx::query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Host')")
        .bind(host)
        .bind(format!("{host}@test.invalid"))
        .execute(&env.db)
        .await
        .unwrap();
    let sid = Uuid::new_v4().to_string();
    let _: () = env
        .redis
        .hset(keys::login_session(&sid), ("user_id", host.to_string()))
        .await
        .unwrap();

    let session = Uuid::new_v4();
    let quiz = quiz();
    let pin = env
        .store
        .create_game(&session.to_string(), &host.to_string(), &quiz)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO game_sessions (id, host_id, pin, quiz_snapshot) VALUES ($1, $2, $3, $4)",
    )
    .bind(session)
    .bind(host)
    .bind(&pin)
    .bind(serde_json::to_value(&quiz).unwrap())
    .execute(&env.db)
    .await
    .unwrap();
    (pin, sid, session)
}

/// A scripted client: sends requests, waits for acks, keeps pushes in order.
struct Bot {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u64,
    pushes: VecDeque<Value>,
}

impl Bot {
    async fn connect(addr: &str, sid: Option<&str>) -> Bot {
        // A local test server: plain ws is fine here.
        // nosemgrep: javascript.lang.security.detect-insecure-websocket.detect-insecure-websocket
        let mut req = format!("ws://{addr}/ws").into_client_request().unwrap();
        if let Some(sid) = sid {
            req.headers_mut()
                .insert("cookie", format!("theme=dark; sid={sid}").parse().unwrap());
        }
        let (ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
        Bot {
            ws,
            next_id: 1,
            pushes: VecDeque::new(),
        }
    }

    async fn read(&mut self) -> Option<Value> {
        loop {
            match tokio::time::timeout(WAIT, self.ws.next())
                .await
                .expect("timed out")
            {
                Some(Ok(Message::Text(t))) => return Some(serde_json::from_str(&t).unwrap()),
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return None,
                Some(Ok(_)) => continue,
            }
        }
    }

    /// Sends `t` with `d` and returns the ack.
    async fn req(&mut self, t: &str, d: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let frame = json!({ "t": t, "id": id, "d": d }).to_string();
        self.ws.send(Message::Text(frame.into())).await.unwrap();
        loop {
            let v = self
                .read()
                .await
                .expect("socket closed while waiting for ack");
            if v["t"] == "ack" && v["id"] == id {
                return v;
            }
            self.pushes.push_back(v);
        }
    }

    async fn ok(&mut self, t: &str, d: Value) -> Value {
        let ack = self.req(t, d.clone()).await;
        assert_eq!(ack["ok"], true, "{t} {d} -> {ack}");
        ack["d"].clone()
    }

    async fn refused(&mut self, t: &str, d: Value) -> String {
        let ack = self.req(t, d.clone()).await;
        assert_eq!(ack["ok"], false, "{t} {d} -> {ack}");
        ack["code"].as_str().unwrap().to_string()
    }

    /// Waits for the next push named `t`, skipping others.
    async fn push(&mut self, t: &str) -> Value {
        if let Some(i) = self.pushes.iter().position(|p| p["t"] == t) {
            return self.pushes.remove(i).unwrap()["d"].clone();
        }
        loop {
            let v = self
                .read()
                .await
                .unwrap_or_else(|| panic!("closed waiting for {t}"));
            if v["t"] == t {
                return v["d"].clone();
            }
            self.pushes.push_back(v);
        }
    }

    async fn closed(&mut self) -> bool {
        loop {
            match self.read().await {
                None => return true,
                Some(v) if v["t"] == "player:kicked" => continue,
                Some(v) => self.pushes.push_back(v),
            }
        }
    }
}

async fn wait_until_shown(store: &LiveStore, starts_at: u64) {
    while store.now_ms().await.unwrap() < starts_at {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn three_bots_play_a_full_game() {
    let env = env_or_skip!();
    let (pin, sid, session) = seed_game(&env).await;

    // Host attaches with the login cookie; others cannot act as host.
    let mut host = Bot::connect(&env.addr, Some(&sid)).await;
    let snap = host.ok("host:attach", json!({ "pin": pin })).await;
    assert_eq!(snap["state"], json!({ "state": "lobby" }));
    let mut stranger = Bot::connect(&env.addr, None).await;
    assert_eq!(
        stranger.refused("host:attach", json!({ "pin": pin })).await,
        "UNAUTHENTICATED"
    );
    assert_eq!(stranger.refused("host:start", json!({})).await, "FORBIDDEN");

    // Three players join.
    let mut bots = Vec::new();
    let mut tokens = Vec::new();
    for (i, nick) in ["An", "Bình", "Chi"].iter().enumerate() {
        let mut b = Bot::connect(&env.addr, None).await;
        let joined = b
            .ok(
                "player:join",
                json!({ "pin": pin, "nickname": nick, "deviceId": format!("dev-{i}") }),
            )
            .await;
        assert_eq!(joined["snapshot"]["me"]["nickname"], *nick);
        tokens.push(joined["token"].as_str().unwrap().to_string());
        bots.push(b);
    }
    assert_eq!(
        stranger
            .refused(
                "player:join",
                json!({ "pin": pin, "nickname": "an", "deviceId": "x" })
            )
            .await,
        "NICK_TAKEN"
    );
    assert_eq!(
        stranger
            .refused(
                "player:join",
                json!({ "pin": pin, "nickname": "A", "deviceId": "x" })
            )
            .await,
        "NICK_INVALID"
    );
    assert_eq!(
        stranger
            .refused(
                "player:join",
                json!({ "pin": "000000", "nickname": "Zed", "deviceId": "x" })
            )
            .await,
        "PIN_INVALID"
    );
    assert_eq!(
        bots[0].refused("host:start", json!({})).await,
        "FORBIDDEN",
        "players cannot host"
    );

    loop {
        let lobby = host.push("lobby:update").await;
        if lobby["count"] == 3 {
            break;
        }
    }

    // Question 0: everyone answers, so it is revealed before the deadline.
    host.ok("host:start", json!({})).await;
    let q0 = bots[0].push("question:start").await;
    assert_eq!(q0["index"], 0);
    assert!(
        q0["options"][0].get("isCorrect").is_none(),
        "no answers leak (SEC-15)"
    );
    for b in bots.iter_mut().skip(1) {
        b.push("question:start").await;
    }
    assert_eq!(
        bots[0]
            .refused("answer:submit", json!({ "index": 0, "optionIds": [1] }))
            .await,
        "EARLY"
    );
    wait_until_shown(&env.store, q0["startsAt"].as_u64().unwrap()).await;

    bots[0]
        .ok("answer:submit", json!({ "index": 0, "optionIds": [1] }))
        .await;
    assert_eq!(
        bots[0]
            .refused("answer:submit", json!({ "index": 0, "optionIds": [2] }))
            .await,
        "DUPLICATE"
    );
    assert_eq!(
        bots[1]
            .refused("answer:submit", json!({ "index": 0, "optionIds": [9] }))
            .await,
        "INVALID"
    );
    bots[1]
        .ok("answer:submit", json!({ "index": 0, "optionIds": [1] }))
        .await;
    bots[2]
        .ok("answer:submit", json!({ "index": 0, "optionIds": [2] }))
        .await;

    let reveal = host.push("question:reveal").await;
    assert_eq!(reveal["correctOptionIds"], json!([1]));
    assert_eq!(
        reveal["distribution"],
        json!([{ "optionId": 1, "count": 2 }, { "optionId": 2, "count": 1 }])
    );
    let r0 = bots[0].push("question:result").await;
    let r2 = bots[2].push("question:result").await;
    assert_eq!(
        (r0["correct"].clone(), r0["rank"].clone()),
        (json!(true), json!(1))
    );
    assert!(r0["points"].as_u64().unwrap() > 500);
    assert_eq!(
        (
            r2["correct"].clone(),
            r2["points"].clone(),
            r2["rank"].clone()
        ),
        (json!(false), json!(0), json!(3))
    );
    bots[1].push("question:result").await;

    // Leaderboard; a double click is refused.
    let reveal_state = json!({ "state": "reveal", "index": 0 });
    host.ok("host:next", json!({ "expect": reveal_state }))
        .await;
    assert_eq!(
        host.refused("host:next", json!({ "expect": reveal_state }))
            .await,
        "STALE"
    );
    let board = bots[2].push("leaderboard:show").await;
    assert_eq!(board["top"][0]["nickname"], "An");
    assert_eq!(board["top"].as_array().unwrap().len(), 3);

    // Question 1. Chi drops and resumes; only An answers, so the deadline
    // sweep reveals it after the 5 s limit.
    host.ok(
        "host:next",
        json!({ "expect": { "state": "leaderboard", "index": 0 } }),
    )
    .await;
    let q1 = bots[0].push("question:start").await;
    assert_eq!(q1["index"], 1);
    drop(bots.pop());
    let mut chi = Bot::connect(&env.addr, None).await;
    let snap = chi
        .ok("player:resume", json!({ "pin": pin, "token": tokens[2] }))
        .await;
    assert_eq!(snap["question"]["index"], 1);
    assert_eq!(snap["me"]["nickname"], "Chi");
    assert_eq!(snap["me"]["answered"], false);
    assert_eq!(
        chi.refused("player:resume", json!({ "pin": pin, "token": "forged" }))
            .await,
        "TOKEN_INVALID"
    );
    bots.push(chi);

    wait_until_shown(&env.store, q1["startsAt"].as_u64().unwrap()).await;
    bots[0]
        .ok("answer:submit", json!({ "index": 1, "optionIds": [2, 1] }))
        .await;
    let reveal = host.push("question:reveal").await;
    assert_eq!(reveal["index"], 1);
    let r = bots[2].push("question:result").await;
    assert_eq!(
        (r["correct"].clone(), r["points"].clone()),
        (json!(false), json!(0))
    );
    assert_eq!(
        bots[0]
            .refused("answer:submit", json!({ "index": 1, "optionIds": [1] }))
            .await,
        "WRONG_STATE"
    );

    // Leaderboard → podium → ended.
    host.ok(
        "host:next",
        json!({ "expect": { "state": "reveal", "index": 1 } }),
    )
    .await;
    host.ok(
        "host:next",
        json!({ "expect": { "state": "leaderboard", "index": 1 } }),
    )
    .await;
    let podium = bots[1].push("game:podium").await;
    assert_eq!(podium["top"][0]["nickname"], "An");
    host.ok("host:next", json!({ "expect": { "state": "podium" } }))
        .await;
    let ended = bots[1].push("game:ended").await;
    assert_eq!(ended["reason"], "finished");

    // Everything is in PostgreSQL.
    let answers: i64 = sqlx::query_scalar("SELECT count(*) FROM answers WHERE session_id = $1")
        .bind(session)
        .fetch_one(&env.db)
        .await
        .unwrap();
    assert_eq!(answers, 4);
    let (status, db_pin, count): (String, Option<String>, i32) =
        sqlx::query_as("SELECT status::text, pin, player_count FROM game_sessions WHERE id = $1")
            .bind(session)
            .fetch_one(&env.db)
            .await
            .unwrap();
    assert_eq!((status.as_str(), db_pin, count), ("ended", None, 3));
    let ranks: Vec<(String, i32)> = sqlx::query_as(
        "SELECT nickname, final_rank FROM session_players WHERE session_id = $1 ORDER BY final_rank",
    )
    .bind(session)
    .fetch_all(&env.db)
    .await
    .unwrap();
    assert_eq!(ranks[0], ("An".to_string(), 1));
    assert_eq!(ranks.len(), 3);
}

#[tokio::test]
async fn kick_lock_and_end_early() {
    let env = env_or_skip!();
    let (pin, sid, session) = seed_game(&env).await;
    let mut host = Bot::connect(&env.addr, Some(&sid)).await;
    host.ok("host:attach", json!({ "pin": pin })).await;

    let mut a = Bot::connect(&env.addr, None).await;
    let pa = a
        .ok(
            "player:join",
            json!({ "pin": pin, "nickname": "Troll", "deviceId": "dev-t" }),
        )
        .await;
    let mut b = Bot::connect(&env.addr, None).await;
    b.ok(
        "player:join",
        json!({ "pin": pin, "nickname": "Good", "deviceId": "dev-g" }),
    )
    .await;

    // Kick: the player is told and disconnected, and cannot rejoin.
    host.ok("host:kick", json!({ "playerId": pa["playerId"] }))
        .await;
    assert!(a.closed().await);
    let mut again = Bot::connect(&env.addr, None).await;
    assert_eq!(
        again
            .refused(
                "player:join",
                json!({ "pin": pin, "nickname": "Troll2", "deviceId": "dev-t" })
            )
            .await,
        "KICKED"
    );

    // Lock: new players are refused with a clear code.
    host.ok("host:lock", json!({ "locked": true })).await;
    assert_eq!(
        again
            .refused(
                "player:join",
                json!({ "pin": pin, "nickname": "Late", "deviceId": "dev-l" })
            )
            .await,
        "LOCKED"
    );
    host.ok("host:lock", json!({ "locked": false })).await;

    // End during a question: the answer given so far is still saved.
    host.ok("host:start", json!({})).await;
    let q = b.push("question:start").await;
    wait_until_shown(&env.store, q["startsAt"].as_u64().unwrap()).await;
    // Only Good is still connected, so this one answer reveals the
    // question at once; the host then ends the game.
    b.ok("answer:submit", json!({ "index": 0, "optionIds": [1] }))
        .await;
    host.ok("host:end", json!({})).await;
    let ended = b.push("game:ended").await;
    assert_eq!(ended["reason"], "host_ended");
    let saved: i64 = sqlx::query_scalar("SELECT count(*) FROM answers WHERE session_id = $1")
        .bind(session)
        .fetch_one(&env.db)
        .await
        .unwrap();
    assert_eq!(saved, 1);
    let kicked: bool = sqlx::query_scalar(
        "SELECT kicked FROM session_players WHERE session_id = $1 AND nickname = 'Troll'",
    )
    .bind(session)
    .fetch_one(&env.db)
    .await
    .unwrap();
    assert!(kicked);
}

#[tokio::test]
async fn clock_sync_and_bad_frames() {
    let env = env_or_skip!();
    let mut bot = Bot::connect(&env.addr, None).await;
    let pong = bot.ok("time:ping", json!({ "clientSentAt": 123.5 })).await;
    assert_eq!(pong["clientSentAt"], 123.5);
    assert!(pong["serverNow"].as_u64().unwrap() > 1_700_000_000_000);

    // Invalid JSON and unknown events are dropped; the socket stays usable.
    bot.ws.send(Message::Text("not json".into())).await.unwrap();
    bot.ws
        .send(Message::Text(r#"{"t":"host:hack","id":99,"d":{}}"#.into()))
        .await
        .unwrap();
    bot.ok("time:ping", json!({ "clientSentAt": 1 })).await;

    // Frames over 4 KB close the socket (SEC-21).
    let big = json!({ "t": "time:ping", "id": 5, "d": { "pad": "x".repeat(5000) } }).to_string();
    bot.ws.send(Message::Text(big.into())).await.unwrap();
    assert!(bot.closed().await);

    // More than 10 messages a second are refused (SEC-19).
    let mut spam = Bot::connect(&env.addr, None).await;
    let mut limited = 0;
    for _ in 0..20 {
        if spam.req("time:ping", json!({ "clientSentAt": 0 })).await["code"] == "RATE_LIMITED" {
            limited += 1;
        }
    }
    assert!(limited >= 5, "only {limited} limited");
}

#[tokio::test]
async fn host_and_players_on_different_servers() {
    // Two independent game servers sharing one Redis, as in production
    // (design section 4): no stickiness, events cross via pub/sub.
    let a = env_or_skip!();
    let b = env_or_skip!();
    let (pin, sid, _) = seed_game(&a).await;

    let mut host = Bot::connect(&a.addr, Some(&sid)).await;
    host.ok("host:attach", json!({ "pin": pin })).await;
    let mut p = Bot::connect(&b.addr, None).await;
    p.ok(
        "player:join",
        json!({ "pin": pin, "nickname": "Remote", "deviceId": "dev-r" }),
    )
    .await;
    let lobby = host.push("lobby:update").await;
    assert_eq!(lobby["players"][0]["nickname"], "Remote");

    host.ok("host:start", json!({})).await;
    let q = p.push("question:start").await;
    wait_until_shown(&b.store, q["startsAt"].as_u64().unwrap()).await;
    p.ok("answer:submit", json!({ "index": 0, "optionIds": [1] }))
        .await;
    let r = p.push("question:result").await;
    assert_eq!(r["correct"], true);
    assert_eq!(host.push("question:reveal").await["index"], 0);
    host.ok("host:end", json!({})).await;
    assert_eq!(p.push("game:ended").await["reason"], "host_ended");
}

#[tokio::test]
async fn suspending_the_host_ends_their_game() {
    let env = env_or_skip!();
    let (pin, sid, session) = seed_game(&env).await;
    let mut host = Bot::connect(&env.addr, Some(&sid)).await;
    host.ok("host:attach", json!({ "pin": pin })).await;
    let mut p = Bot::connect(&env.addr, None).await;
    p.ok(
        "player:join",
        json!({ "pin": pin, "nickname": "Lan", "deviceId": "dev-s" }),
    )
    .await;

    // FR-AD-01: an admin suspends the host.
    sqlx::query(
        "UPDATE users SET status = 'suspended' \
         WHERE id = (SELECT host_id FROM game_sessions WHERE id = $1)",
    )
    .bind(session)
    .execute(&env.db)
    .await
    .unwrap();
    let ended = tokio::time::timeout(Duration::from_secs(20), p.push("game:ended"))
        .await
        .expect("game ended within the sweep interval");
    assert_eq!(ended["reason"], "suspended");
    let status: String = sqlx::query_scalar("SELECT status::text FROM game_sessions WHERE id = $1")
        .bind(session)
        .fetch_one(&env.db)
        .await
        .unwrap();
    assert_eq!(status, "ended");
}

/// SEC-26: a page on another site cannot open a game socket with the
/// host's cookie; the page's own origin can.
#[tokio::test]
async fn sockets_from_other_sites_are_refused() {
    let env = env_or_skip!();
    let open = |origin: String| {
        let addr = env.addr.clone();
        async move {
            let mut req = format!("ws://{addr}/ws").into_client_request().unwrap();
            req.headers_mut().insert("origin", origin.parse().unwrap());
            tokio_tungstenite::connect_async(req).await
        }
    };
    match open("https://evil.example".into()).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(res)) => {
            assert_eq!(res.status(), 403)
        }
        other => panic!("expected 403, got {:?}", other.map(|_| ())),
    }
    assert!(open(format!("http://{}", env.addr)).await.is_ok());
}
