//! WebSocket load test (NFR-02, NFR-03; design section 11).
//!
//! Plays many full games at once against a running stack and reports how
//! long live events take to reach players:
//!
//! - `question_start`: host clicks (`host:start` / `host:next`) until each
//!   player has the question. It carries a 3 s "get ready" countdown, so
//!   what players notice is `questionsAfterCountdown`: questions that
//!   arrived after their options should already be on screen.
//! - `leaderboard`: host clicks until each player has the leaderboard.
//!   NFR-02 wants p95 ≤ 250 ms and max ≤ 500 ms for this and the reveal.
//! - `answer_ack`: a player sends an answer until the server accepts it.
//! - `reveal_result`: the last answer of a question is accepted until each
//!   player has their result (the reveal is triggered by that answer).
//!
//! Each game gets its own host account, made directly in PostgreSQL and
//! Redis like the sign-in flow would, then goes through the real API
//! (`POST /quizzes`, publish, `POST /games`), so the MAX_ACTIVE_GAMES cap is
//! exercised too. Use a test database: the accounts are named
//! `loadtest-*@test.invalid` and can be removed afterwards with
//! `--cleanup`.
//!
//! ```sh
//! cargo run --release --example loadtest -- \
//!   --api http://127.0.0.1:8080 --ws ws://127.0.0.1:8081/ws \
//!   --games 50 --players 200 --questions 5
//! ```
//!
//! The load generator needs one open file per player; raise `ulimit -n`
//! or run several copies with fewer games each (`--tag` keeps their
//! nicknames apart).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use fred::prelude::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

#[derive(Parser, Debug, Clone)]
struct Args {
    /// Base URL of the API (or Caddy).
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    api: String,
    /// Game socket URL(s), comma separated; games are spread across them.
    #[arg(long, default_value = "ws://127.0.0.1:8081/ws")]
    ws: String,
    #[arg(long, default_value_t = 10)]
    games: usize,
    #[arg(long, default_value_t = 200)]
    players: usize,
    #[arg(long, default_value_t = 5)]
    questions: usize,
    /// Spread game starts over this many seconds.
    #[arg(long, default_value_t = 10)]
    ramp_secs: u64,
    /// Told apart when several load generators run at once.
    #[arg(long, default_value = "a")]
    tag: String,
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,
    #[arg(long, env = "REDIS_URL")]
    redis_url: String,
    /// Write the summary as JSON here as well.
    #[arg(long)]
    json: Option<String>,
    /// Remove the load test accounts and their games, then exit.
    #[arg(long)]
    cleanup: bool,
}

#[derive(Default)]
struct Metrics {
    samples: Mutex<HashMap<&'static str, Vec<u32>>>,
    errors: Mutex<HashMap<String, usize>>,
    joined: AtomicUsize,
    finished_games: AtomicUsize,
    late_questions: AtomicUsize,
}

impl Metrics {
    fn record(&self, name: &'static str, ms: u32) {
        self.samples
            .lock()
            .unwrap()
            .entry(name)
            .or_default()
            .push(ms);
    }
    fn error(&self, what: impl Into<String>) {
        *self.errors.lock().unwrap().entry(what.into()).or_default() += 1;
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// The host's last click and the last accepted answer, per game, in ms
/// since the epoch: the load generator, API and game servers share a clock
/// when they run on one machine; otherwise sync the clocks with NTP.
#[derive(Default)]
struct GameClock {
    host_sent: AtomicU64,
    last_answer: AtomicU64,
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(url: &str, sid: Option<&str>) -> anyhow::Result<Ws> {
    let mut req = url.into_client_request()?;
    if let Some(sid) = sid {
        req.headers_mut()
            .insert("cookie", format!("sid={sid}").parse()?);
    }
    let (ws, _) = tokio_tungstenite::connect_async(req).await?;
    Ok(ws)
}

async fn send(ws: &mut Ws, id: u64, t: &str, d: Value) -> anyhow::Result<()> {
    ws.send(Message::text(
        json!({ "t": t, "id": id, "d": d }).to_string(),
    ))
    .await?;
    Ok(())
}

/// Next JSON frame, or an error after `wait`.
async fn recv(ws: &mut Ws, wait: Duration) -> anyhow::Result<Value> {
    loop {
        let msg = tokio::time::timeout(wait, ws.next())
            .await
            .map_err(|_| anyhow::anyhow!("timeout"))?
            .ok_or_else(|| anyhow::anyhow!("closed"))??;
        if let Message::Text(t) = msg {
            return Ok(serde_json::from_str(&t)?);
        }
    }
}

/// Waits for the ack of request `id`, dropping pushes on the way.
async fn ack(ws: &mut Ws, id: u64) -> anyhow::Result<Value> {
    loop {
        let v = recv(ws, Duration::from_secs(30)).await?;
        if v["t"] == "ack" && v["id"] == id {
            if v["ok"] == true {
                return Ok(v["d"].clone());
            }
            anyhow::bail!("refused {}", v["code"]);
        }
    }
}

async fn wait_push(ws: &mut Ws, t: &str, wait: Duration) -> anyhow::Result<Value> {
    loop {
        let v = recv(ws, wait).await?;
        if v["t"] == t {
            return Ok(v["d"].clone());
        }
    }
}

struct Host {
    sid: String,
    csrf: String,
}

async fn make_host(db: &sqlx::PgPool, redis: &Client, tag: &str, i: usize) -> anyhow::Result<Host> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(format!("loadtest-{tag}-{i}-{id}@test.invalid"))
        .bind(format!("Load {tag}{i}"))
        .execute(db)
        .await?;
    let sid = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let csrf = Uuid::new_v4().simple().to_string();
    let key = format!("session:{sid}");
    let _: () = redis
        .hset(&key, [("user_id", id.to_string()), ("csrf", csrf.clone())])
        .await?;
    let _: () = redis.expire(&key, 3 * 3600, None).await?;
    Ok(Host { sid, csrf })
}

/// Creates and publishes a quiz through the API and opens a game.
async fn open_game(http: &reqwest::Client, args: &Args, host: &Host) -> anyhow::Result<String> {
    let call = |method: reqwest::Method, path: &str| {
        http.request(method, format!("{}/api/v1{path}", args.api))
            .header("cookie", format!("sid={}", host.sid))
            .header("x-csrf-token", &host.csrf)
    };
    let questions: Vec<Value> = (0..args.questions)
        .map(|i| {
            json!({
                "type": "single",
                "text": format!("Câu {}", i + 1),
                "timeLimitS": 10,
                "points": 1000,
                "options": [
                    { "text": "Đúng", "correct": true },
                    { "text": "Sai", "correct": false },
                    { "text": "Không biết", "correct": false },
                    { "text": "Bỏ qua", "correct": false },
                ],
            })
        })
        .collect();
    let quiz: Value = call(reqwest::Method::POST, "/quizzes")
        .json(&json!({ "title": "Load test", "description": "", "questions": questions }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let quiz_id = quiz["id"].as_str().unwrap_or_default().to_string();
    call(
        reqwest::Method::POST,
        &format!("/quizzes/{quiz_id}/publish"),
    )
    .send()
    .await?
    .error_for_status()?;
    let res = call(reqwest::Method::POST, "/games")
        .json(&json!({ "quizId": quiz_id }))
        .send()
        .await?;
    if !res.status().is_success() {
        let status = res.status();
        let body: Value = res.json().await.unwrap_or_default();
        anyhow::bail!("POST /games {status} {}", body["error"]["code"]);
    }
    let game: Value = res.json().await?;
    Ok(game["pin"].as_str().unwrap_or_default().to_string())
}

/// One player: join, then answer every question at a human-ish moment.
async fn player(
    ws_url: String,
    pin: String,
    nick: String,
    clock: Arc<GameClock>,
    m: Arc<Metrics>,
) -> anyhow::Result<()> {
    let mut ws = connect(&ws_url, None).await?;
    send(
        &mut ws,
        1,
        "player:join",
        json!({ "pin": pin, "nickname": nick, "deviceId": nick }),
    )
    .await?;
    ack(&mut ws, 1).await?;
    m.joined.fetch_add(1, Ordering::Relaxed);
    let mut next_id = 2;
    loop {
        let v = recv(&mut ws, Duration::from_secs(120)).await?;
        let got = now_ms();
        match v["t"].as_str().unwrap_or("") {
            "question:start" => {
                let sent = clock.host_sent.load(Ordering::Relaxed);
                m.record("question_start", got.saturating_sub(sent) as u32);
                let d = &v["d"];
                let index = d["index"].as_u64().unwrap_or(0);
                let starts_at = d["startsAt"].as_u64().unwrap_or(got);
                // Options show at startsAt, after a 3 s "get ready"
                // countdown; a question that arrives later than that is
                // late on screen (design section 4).
                if got > starts_at {
                    m.late_questions.fetch_add(1, Ordering::Relaxed);
                }
                // Answer 0.3 to 4 s after the options show, mostly right.
                let think = 300 + rand::random_range(0..3700u64);
                let at = starts_at + think;
                if at > now_ms() {
                    tokio::time::sleep(Duration::from_millis(at - now_ms())).await;
                }
                let option = if rand::random_range(0..10) < 7 { 1 } else { 2 };
                let id = next_id;
                next_id += 1;
                let t0 = Instant::now();
                send(
                    &mut ws,
                    id,
                    "answer:submit",
                    json!({ "index": index, "optionIds": [option] }),
                )
                .await?;
                // Pushes that arrive before the ack are handled in order.
                loop {
                    let v = recv(&mut ws, Duration::from_secs(30)).await?;
                    if v["t"] == "ack" && v["id"] == id {
                        m.record("answer_ack", t0.elapsed().as_millis() as u32);
                        if v["ok"] != true {
                            m.error(format!("answer refused {}", v["code"]));
                        }
                        clock.last_answer.fetch_max(now_ms(), Ordering::Relaxed);
                        break;
                    }
                    if v["t"] == "question:result" {
                        m.error("result before ack");
                        break;
                    }
                }
            }
            "question:result" => {
                let last = clock.last_answer.load(Ordering::Relaxed);
                if v["d"]["answered"] == true && last > 0 {
                    m.record("reveal_result", got.saturating_sub(last) as u32);
                }
            }
            "leaderboard:show" => {
                let sent = clock.host_sent.load(Ordering::Relaxed);
                m.record("leaderboard", got.saturating_sub(sent) as u32);
            }
            "game:podium" | "game:ended" => return Ok(()),
            "error" => m.error(format!("push error {}", v["d"]["code"])),
            _ => {}
        }
    }
}

/// A host click, timed from the moment it is sent.
async fn click(
    ws: &mut Ws,
    clock: &GameClock,
    id: &mut u64,
    t: &str,
    d: Value,
) -> anyhow::Result<Value> {
    clock.host_sent.store(now_ms(), Ordering::Relaxed);
    clock.last_answer.store(0, Ordering::Relaxed);
    *id += 1;
    send(ws, *id, t, d).await?;
    ack(ws, *id).await
}

/// One game: host attaches, waits for everyone, then clicks through.
async fn game(
    args: Arc<Args>,
    i: usize,
    pin: String,
    host: Host,
    m: Arc<Metrics>,
) -> anyhow::Result<()> {
    let urls: Vec<&str> = args.ws.split(',').collect();
    let ws_url = urls[i % urls.len()].to_string();
    let clock = Arc::new(GameClock::default());
    let mut ws = connect(&ws_url, Some(&host.sid)).await?;
    send(&mut ws, 1, "host:attach", json!({ "pin": pin })).await?;
    ack(&mut ws, 1).await?;

    let mut players = Vec::new();
    for p in 0..args.players {
        let nick = format!("Bot {}{i}x{p}", args.tag);
        let (url, pin, clock, m) = (ws_url.clone(), pin.clone(), clock.clone(), m.clone());
        players.push(tokio::spawn(async move {
            if let Err(e) = player(url, pin, nick, clock, m.clone()).await {
                m.error(format!("player: {e}"));
            }
        }));
        // Players arrive over a few seconds, not all in one instant.
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // Wait until everyone is in the lobby.
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let d = wait_push(&mut ws, "lobby:update", Duration::from_secs(120)).await?;
        if d["count"].as_u64().unwrap_or(0) as usize >= args.players || Instant::now() > deadline {
            break;
        }
    }
    tokio::time::sleep(Duration::from_secs(1)).await;

    let mut id = 2;
    click(&mut ws, &clock, &mut id, "host:start", json!({})).await?;
    for q in 0..args.questions {
        wait_push(&mut ws, "question:reveal", Duration::from_secs(60)).await?;
        tokio::time::sleep(Duration::from_secs(2)).await;
        click(
            &mut ws,
            &clock,
            &mut id,
            "host:next",
            json!({ "expect": { "state": "reveal", "index": q } }),
        )
        .await?;
        tokio::time::sleep(Duration::from_secs(2)).await;
        click(
            &mut ws,
            &clock,
            &mut id,
            "host:next",
            json!({ "expect": { "state": "leaderboard", "index": q } }),
        )
        .await?;
    }
    for p in players {
        let _ = p.await;
    }
    click(
        &mut ws,
        &clock,
        &mut id,
        "host:next",
        json!({ "expect": { "state": "podium" } }),
    )
    .await?;
    m.finished_games.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

fn summary(m: &Metrics) -> Value {
    let mut out = serde_json::Map::new();
    for (name, v) in m.samples.lock().unwrap().iter_mut() {
        v.sort_unstable();
        let pct = |p: f64| v[((v.len() as f64 * p).ceil() as usize).clamp(1, v.len()) - 1];
        out.insert(
            name.to_string(),
            json!({
                "count": v.len(),
                "p50": pct(0.50),
                "p95": pct(0.95),
                "p99": pct(0.99),
                "max": v[v.len() - 1],
            }),
        );
    }
    json!({
        "latencyMs": out,
        "questionsAfterCountdown": m.late_questions.load(Ordering::Relaxed),
        "errors": *m.errors.lock().unwrap(),
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Arc::new(Args::parse());
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&args.database_url)
        .await?;
    if args.cleanup {
        let hosts = "SELECT id FROM users WHERE email LIKE 'loadtest-%@test.invalid'";
        for table in [("game_sessions", "host_id"), ("quizzes", "owner_id")] {
            sqlx::query(&format!(
                "DELETE FROM {} WHERE {} IN ({hosts})",
                table.0, table.1
            ))
            .execute(&db)
            .await?;
        }
        let gone = sqlx::query("DELETE FROM users WHERE email LIKE 'loadtest-%@test.invalid'")
            .execute(&db)
            .await?;
        println!(
            "removed {} load test accounts and their games",
            gone.rows_affected()
        );
        return Ok(());
    }
    let redis = Client::new(Config::from_url(&args.redis_url)?, None, None, None);
    redis.init().await?;
    let http = reqwest::Client::new();
    let m = Arc::new(Metrics::default());

    println!(
        "opening {} games of {} players, {} questions each",
        args.games, args.players, args.questions
    );
    let started = Instant::now();
    let mut games = Vec::new();
    for i in 0..args.games {
        let host = make_host(&db, &redis, &args.tag, i).await?;
        let pin = match open_game(&http, &args, &host).await {
            Ok(pin) => pin,
            Err(e) => {
                m.error(format!("open game: {e}"));
                continue;
            }
        };
        let (args2, m2) = (args.clone(), m.clone());
        games.push(tokio::spawn(async move {
            if let Err(e) = game(args2, i, pin, host, m2.clone()).await {
                m2.error(format!("host: {e}"));
            }
        }));
        if args.games > 1 {
            tokio::time::sleep(Duration::from_millis(
                args.ramp_secs * 1000 / args.games as u64,
            ))
            .await;
        }
    }
    let progress = {
        let m = m.clone();
        let total = args.games * args.players;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                println!(
                    "  {}s: {}/{} players joined, {} games finished",
                    started.elapsed().as_secs(),
                    m.joined.load(Ordering::Relaxed),
                    total,
                    m.finished_games.load(Ordering::Relaxed)
                );
            }
        })
    };
    for g in games {
        let _ = g.await;
    }
    progress.abort();
    let s = summary(&m);
    println!(
        "finished {}/{} games in {} s",
        m.finished_games.load(Ordering::Relaxed),
        args.games,
        started.elapsed().as_secs()
    );
    println!("{}", serde_json::to_string_pretty(&s)?);
    if let Some(path) = &args.json {
        std::fs::write(path, serde_json::to_string_pretty(&s)?)?;
    }
    Ok(())
}
