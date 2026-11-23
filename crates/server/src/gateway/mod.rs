//! The live game WebSocket endpoint, `/ws` (design section 7).
//!
//! One socket per screen. A socket starts anonymous and becomes a host
//! socket (`host:attach`, login cookie must own the game) or a player socket
//! (`player:join` or `player:resume`, bound to the player id from the
//! token, never from the payload; SEC-07).

pub mod game;
pub mod hub;

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use myquiz_protocol::ws::*;
use tokio::sync::broadcast;

use game::{Code, GameService};
use hub::Delivery;

/// Largest accepted frame (SEC-21).
pub const MAX_FRAME_BYTES: usize = 4 * 1024;
const SOCKET_BUFFER_BYTES: usize = 8 * 1024;
/// A client that stops reading is dropped rather than buffered forever.
const MAX_WRITE_BUFFER_BYTES: usize = 1024 * 1024;
/// Messages per second per socket (SEC-19).
pub const RATE_PER_SEC: f64 = 10.0;
const PING_EVERY: Duration = Duration::from_secs(25);
const IDLE_LIMIT: Duration = Duration::from_secs(60);
const SWEEP_EVERY: Duration = Duration::from_millis(250);

/// Web origins allowed to open a game socket besides the page's own host
/// (PUBLIC_URL and JOIN_URL), as `scheme://host[:port]`, lower case.
#[derive(Clone, Default)]
pub struct AllowedOrigins(Arc<Vec<String>>);

impl AllowedOrigins {
    pub fn new<'a>(urls: impl IntoIterator<Item = &'a str>) -> Self {
        AllowedOrigins(Arc::new(urls.into_iter().filter_map(origin_of).collect()))
    }
}

/// `https://Quiz.example.edu.vn/path` -> `https://quiz.example.edu.vn`.
fn origin_of(url: &str) -> Option<String> {
    let url = url.trim().to_ascii_lowercase();
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    (!authority.is_empty()).then(|| format!("{scheme}://{authority}"))
}

/// Cross-site WebSocket hijacking guard (SEC-26). Browsers always send
/// `Origin` on a socket, and cookies ride along, so a page on another site
/// (even another subdomain of the same university) could otherwise drive a
/// signed-in host's game. Clients without `Origin` are not browsers and
/// carry no cookies of a victim, so they pass.
fn origin_allowed(headers: &HeaderMap, allowed: &AllowedOrigins) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Some(origin) = origin.to_str().ok().and_then(origin_of) else {
        return false;
    };
    if allowed.0.contains(&origin) {
        return true;
    }
    // Same origin as the page that asked: Caddy and the Vite proxy keep the
    // browser's Host header (Caddy also adds X-Forwarded-Host).
    let authority = origin.split_once("://").map_or("", |(_, a)| a);
    [header::HOST.as_str(), "x-forwarded-host"]
        .iter()
        .filter_map(|h| headers.get(*h)?.to_str().ok())
        .any(|h| h.trim().eq_ignore_ascii_case(authority))
}

#[derive(Clone)]
struct WsState {
    service: Arc<GameService>,
    origins: AllowedOrigins,
}

pub fn router(service: Arc<GameService>, origins: AllowedOrigins) -> Router {
    Router::new()
        .route("/ws", get(upgrade))
        .with_state(WsState { service, origins })
}

async fn upgrade(
    State(WsState { service, origins }): State<WsState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_allowed(&headers, &origins) {
        tracing::warn!(origin = ?headers.get(header::ORIGIN), "socket from another site refused");
        return StatusCode::FORBIDDEN.into_response();
    }
    // The login cookie identifies hosts and signed-in players. The session
    // itself is created by the auth module (SEC-04).
    let sid = cookie(&headers, "sid");
    ws.max_message_size(MAX_FRAME_BYTES)
        .max_frame_size(MAX_FRAME_BYTES)
        // The 128 KiB default buffers cost about 150 KB per socket (load
        // test, 10,000 sockets); clients send at most 4 KiB and our pushes
        // are small, so 8 KiB buffers do. The write buffer still grows for
        // one large message, up to the cap.
        .read_buffer_size(SOCKET_BUFFER_BYTES)
        .write_buffer_size(SOCKET_BUFFER_BYTES)
        .max_write_buffer_size(MAX_WRITE_BUFFER_BYTES)
        .on_upgrade(move |socket| async move {
            let user = match sid {
                Some(sid) => service.store.login_user(&sid).await.unwrap_or(None),
                None => None,
            };
            Connection::new(service, user).run(socket).await;
        })
}

pub(crate) fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

/// The deadline sweep (design section 4): every worker checks every 250 ms
/// for questions past their end time. The Redis reveal script lets only one
/// worker act on each.
pub fn spawn_sweep(service: Arc<GameService>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(SWEEP_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let due = match service.store.due(100).await {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!(error = %e, "deadline sweep failed");
                    continue;
                }
            };
            for pin in due {
                let service = service.clone();
                tokio::spawn(async move {
                    let run = async {
                        if let Some(meta) = service.store.meta(&pin).await? {
                            if let myquiz_engine::GameState::Question { index } = meta.state {
                                service.finish_question(&pin, index).await?;
                            }
                        }
                        anyhow::Ok(())
                    };
                    if let Err(e) = run.await {
                        tracing::error!(error = %e, pin, "reveal at deadline failed");
                    }
                });
            }
        }
    });
}

/// FR-AD-01: games hosted by a suspended user end within this long.
const SUSPENDED_SWEEP_EVERY: Duration = Duration::from_secs(10);

/// Ends running games whose host has been suspended. Every game server
/// runs it; `end_game` makes sure each game ends once.
pub fn spawn_suspended_sweep(service: Arc<GameService>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(SUSPENDED_SWEEP_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let pins: Result<Vec<String>, _> = sqlx::query_scalar(
                "SELECT s.pin FROM game_sessions s JOIN users u ON u.id = s.host_id \
                 WHERE s.status <> 'ended' AND s.pin IS NOT NULL \
                   AND (u.status = 'suspended' OR u.deleted_at IS NOT NULL)",
            )
            .fetch_all(&service.db)
            .await;
            match pins {
                Ok(pins) => {
                    for pin in pins {
                        let pin = pin.trim().to_string();
                        if let Err(e) = service
                            .end_game(&pin, myquiz_protocol::ws::EndReason::Suspended)
                            .await
                        {
                            tracing::warn!(error = %e, pin, "ending a suspended host's game failed");
                        } else {
                            tracing::info!(pin, "ended a game of a suspended host");
                        }
                    }
                }
                Err(e) => tracing::warn!(error = %e, "suspended-host sweep failed"),
            }
        }
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Who {
    Anonymous,
    Host { pin: String },
    Player { pin: String, id: String },
}

impl Who {
    fn pin(&self) -> Option<&str> {
        match self {
            Who::Anonymous => None,
            Who::Host { pin } | Who::Player { pin, .. } => Some(pin),
        }
    }
}

/// Token bucket for SEC-19.
struct Bucket {
    tokens: f64,
    last: Instant,
}

impl Bucket {
    fn new() -> Self {
        Self {
            tokens: RATE_PER_SEC,
            last: Instant::now(),
        }
    }

    fn take(&mut self) -> bool {
        let now = Instant::now();
        let refill = now.duration_since(self.last).as_secs_f64() * RATE_PER_SEC;
        self.tokens = (self.tokens + refill).min(RATE_PER_SEC);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

struct Connection {
    service: Arc<GameService>,
    user: Option<String>,
    who: Who,
    room: Option<broadcast::Receiver<Arc<Delivery>>>,
    bucket: Bucket,
    /// Close after sending this frame (kicked).
    closing: bool,
}

enum Reply {
    None,
    Ack(Ack),
}

impl Connection {
    fn new(service: Arc<GameService>, user: Option<String>) -> Self {
        Self {
            service,
            user,
            who: Who::Anonymous,
            room: None,
            bucket: Bucket::new(),
            closing: false,
        }
    }

    async fn run(mut self, mut socket: WebSocket) {
        let mut ping = tokio::time::interval(PING_EVERY);
        ping.tick().await;
        let mut last_seen = Instant::now();

        loop {
            let room_msg = async {
                match self.room.as_mut() {
                    Some(rx) => Some(rx.recv().await),
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                frame = socket.recv() => {
                    let Some(Ok(frame)) = frame else { break };
                    last_seen = Instant::now();
                    match frame {
                        Message::Text(text) => {
                            if let Reply::Ack(ack) = self.handle(text.as_str()).await {
                                let json = serde_json::to_string(&ack).expect("ack");
                                if socket.send(Message::Text(json.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Message::Close(_) => break,
                        _ => {}
                    }
                }
                msg = room_msg => {
                    match msg {
                        Some(Ok(d)) => {
                            let (is_host, pid) = match &self.who {
                                Who::Host { .. } => (true, None),
                                Who::Player { id, .. } => (false, Some(id.as_str())),
                                Who::Anonymous => (false, None),
                            };
                            if let Some(text) = d.text_for(is_host, pid) {
                                if matches!(*d, Delivery::Player { .. }) && text.contains("player:kicked") {
                                    self.closing = true;
                                }
                                if socket.send(Message::Text(text.to_string().into())).await.is_err() {
                                    break;
                                }
                                if self.closing {
                                    break;
                                }
                            }
                        }
                        // Too slow to keep up: drop the socket, the client
                        // reconnects and resumes from a snapshot.
                        Some(Err(broadcast::error::RecvError::Lagged(_))) => break,
                        _ => break,
                    }
                }
                _ = ping.tick() => {
                    if last_seen.elapsed() > IDLE_LIMIT {
                        break;
                    }
                    if socket.send(Message::Ping(Default::default())).await.is_err() {
                        break;
                    }
                }
            }
        }
        self.unbind().await;
    }

    async fn bind(&mut self, who: Who) -> anyhow::Result<()> {
        if self.who == who {
            return Ok(());
        }
        self.unbind().await;
        let pin = who.pin().expect("bound").to_string();
        self.room = Some(self.service.hub.enter(&pin).await?);
        if let Who::Player { id, .. } = &who {
            self.service.player_online(&pin, id, true).await?;
        }
        self.who = who;
        Ok(())
    }

    async fn unbind(&mut self) {
        let who = std::mem::replace(&mut self.who, Who::Anonymous);
        self.room = None;
        if let Some(pin) = who.pin() {
            self.service.hub.leave(pin).await;
        }
        if let Who::Player { pin, id } = &who {
            if let Err(e) = self.service.player_online(pin, id, false).await {
                tracing::warn!(error = %e, "marking player offline failed");
            }
        }
    }

    async fn handle(&mut self, text: &str) -> Reply {
        // Every frame costs a token, valid or not (SEC-19).
        if !self.bucket.take() {
            #[derive(serde::Deserialize)]
            struct IdOnly {
                id: Option<u64>,
            }
            return match serde_json::from_str::<IdOnly>(text) {
                Ok(IdOnly { id: Some(id) }) => Reply::Ack(Ack::err(id, "RATE_LIMITED")),
                _ => Reply::None,
            };
        }
        let frame: Inbound = match serde_json::from_str(text) {
            Ok(f) => f,
            Err(e) => {
                tracing::debug!(error = %e, "invalid frame dropped");
                return Reply::None;
            }
        };
        let id = frame.id;
        let result = self.dispatch(frame.msg).await;
        let Some(id) = id else { return Reply::None };
        Reply::Ack(match result {
            Ok(Ok(d)) => Ack::ok(id, d),
            Ok(Err(code)) => Ack::err(id, code),
            Err(e) => {
                tracing::error!(error = %e, "request failed");
                Ack::err(id, "INTERNAL")
            }
        })
    }

    async fn dispatch(
        &mut self,
        msg: ClientMessage,
    ) -> anyhow::Result<Result<serde_json::Value, Code>> {
        use serde_json::{json, to_value};
        let svc = self.service.clone();
        let host_pin = match &self.who {
            Who::Host { pin } => Some(pin.clone()),
            _ => None,
        };
        let player = match &self.who {
            Who::Player { pin, id } => Some((pin.clone(), id.clone())),
            _ => None,
        };
        let ok = || Ok(json!({}));

        Ok(match msg {
            ClientMessage::TimePing(p) => Ok(to_value(TimePong {
                client_sent_at: p.client_sent_at,
                server_now: svc.store.now_ms().await?,
            })?),

            ClientMessage::HostAttach(a) => {
                let Some(user) = self.user.clone() else {
                    return Ok(Err("UNAUTHENTICATED"));
                };
                match svc.attach_host(&user, &a.pin).await? {
                    Ok(snap) => {
                        self.bind(Who::Host { pin: a.pin }).await?;
                        Ok(to_value(snap)?)
                    }
                    Err(c) => Err(c),
                }
            }

            ClientMessage::PlayerJoin(j) => {
                if self.who != Who::Anonymous {
                    return Ok(Err("ALREADY_JOINED"));
                }
                match svc.join(&j, self.user.clone()).await? {
                    Ok(joined) => {
                        self.bind(Who::Player {
                            pin: j.pin,
                            id: joined.player_id.clone(),
                        })
                        .await?;
                        Ok(to_value(joined)?)
                    }
                    Err(c) => Err(c),
                }
            }

            ClientMessage::PlayerResume(r) => match svc.resume(&r).await? {
                Ok((id, snap)) => {
                    self.bind(Who::Player { pin: r.pin, id }).await?;
                    Ok(to_value(snap)?)
                }
                Err(c) => Err(c),
            },

            ClientMessage::AnswerSubmit(a) => match player {
                Some((pin, id)) => match svc.answer(&pin, &id, a.index, &a.submission).await? {
                    Ok(()) => Ok(to_value(AnswerAccepted { accepted: true })?),
                    Err(c) => Err(c),
                },
                None => Err("FORBIDDEN"),
            },

            // Everything below is host only (SEC-07).
            other => {
                let Some(pin) = host_pin else {
                    return Ok(Err("FORBIDDEN"));
                };
                match other {
                    ClientMessage::HostLock(l) => {
                        svc.lock(&pin, l.locked).await?;
                        ok()
                    }
                    ClientMessage::HostKick(k) => {
                        svc.kick(&pin, &k.player_id).await?.map(|_| json!({}))
                    }
                    ClientMessage::HostStart(_) => svc.start(&pin).await?.map(|_| json!({})),
                    ClientMessage::HostNext(n) => {
                        svc.next(&pin, n.expect).await?.map(|_| json!({}))
                    }
                    ClientMessage::HostSkip(s) => svc.skip(&pin, s.index).await?.map(|_| json!({})),
                    ClientMessage::HostEnd(_) => {
                        svc.end_game(&pin, EndReason::HostEnded).await?;
                        ok()
                    }
                    _ => unreachable!("handled above"),
                }
            }
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod origin_tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    #[test]
    fn sockets_from_other_sites_are_refused() {
        let allowed = AllowedOrigins::new(["https://Quiz.UIT.edu.vn/", "http://192.168.1.20:8080"]);
        let ok = |h: &[(&str, &str)]| origin_allowed(&headers(h), &allowed);
        // Not a browser: no Origin, no victim cookies.
        assert!(ok(&[("host", "game:8080")]));
        // PUBLIC_URL and JOIN_URL, whatever Host the proxy passes on.
        assert!(ok(&[
            ("origin", "https://quiz.uit.edu.vn"),
            ("host", "game:8080")
        ]));
        assert!(ok(&[("origin", "http://192.168.1.20:8080"), ("host", "x")]));
        // Same origin as the page (Vite in development, Caddy's forwarded host).
        assert!(ok(&[
            ("origin", "http://localhost:5173"),
            ("host", "localhost:5173")
        ]));
        assert!(ok(&[
            ("origin", "https://other.example"),
            ("host", "game:8080"),
            ("x-forwarded-host", "other.example"),
        ]));
        // Another subdomain, another port, another scheme's host, garbage.
        assert!(!ok(&[
            ("origin", "https://evil.uit.edu.vn"),
            ("host", "quiz.uit.edu.vn")
        ]));
        assert!(!ok(&[
            ("origin", "http://192.168.1.20:9999"),
            ("host", "game:8080")
        ]));
        assert!(!ok(&[("origin", "null"), ("host", "game:8080")]));
    }
}
