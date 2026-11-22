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
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use myquiz_protocol::ws::*;
use tokio::sync::broadcast;

use game::{Code, GameService};
use hub::Delivery;

/// Largest accepted frame (SEC-21).
pub const MAX_FRAME_BYTES: usize = 4 * 1024;
/// Messages per second per socket (SEC-19).
pub const RATE_PER_SEC: f64 = 10.0;
const PING_EVERY: Duration = Duration::from_secs(25);
const IDLE_LIMIT: Duration = Duration::from_secs(60);
const SWEEP_EVERY: Duration = Duration::from_millis(250);

pub fn router(service: Arc<GameService>) -> Router {
    Router::new().route("/ws", get(upgrade)).with_state(service)
}

async fn upgrade(
    State(service): State<Arc<GameService>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    // The login cookie identifies hosts and signed-in players. The session
    // itself is created by the auth module (SEC-04).
    let sid = cookie(&headers, "sid");
    ws.max_message_size(MAX_FRAME_BYTES)
        .max_frame_size(MAX_FRAME_BYTES)
        .on_upgrade(move |socket| async move {
            let user = match sid {
                Some(sid) => service.store.login_user(&sid).await.unwrap_or(None),
                None => None,
            };
            Connection::new(service, user).run(socket).await;
        })
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
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
