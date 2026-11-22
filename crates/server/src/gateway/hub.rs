//! Fan-out of game events across servers (design section 4).
//!
//! Each server keeps one local broadcast channel per game it has sockets in,
//! and subscribes to the Redis channel `ch:<pin>` only while it does.
//! Whichever server runs a state change publishes the event once; every
//! server holding sockets in that game delivers it to its own sockets.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use fred::clients::SubscriberClient;
use fred::prelude::*;
use myquiz_protocol::ws::ServerPush;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

const ROOM_CAPACITY: usize = 1024;

/// Who on a game's sockets receives a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "to", rename_all = "snake_case")]
pub enum Delivery {
    /// Host and every player.
    All { text: Arc<str> },
    /// The host's screen only.
    Host { text: Arc<str> },
    /// One message per player, by player id; players not listed get nothing.
    Each { texts: HashMap<String, Arc<str>> },
    /// One player (e.g. `player:kicked`).
    Player { id: String, text: Arc<str> },
}

impl Delivery {
    /// The text for a socket, given whether it is the host and its player id.
    pub fn text_for(&self, is_host: bool, player_id: Option<&str>) -> Option<&Arc<str>> {
        match self {
            Delivery::All { text } => Some(text),
            Delivery::Host { text } => is_host.then_some(text),
            Delivery::Each { texts } => player_id.and_then(|p| texts.get(p)),
            Delivery::Player { id, text } => (player_id == Some(id.as_str())).then_some(text),
        }
    }
}

pub fn encode(push: &ServerPush) -> Arc<str> {
    serde_json::to_string(push)
        .expect("serialisable push")
        .into()
}

struct Room {
    tx: broadcast::Sender<Arc<Delivery>>,
    members: usize,
}

pub struct Hub {
    redis: Pool,
    sub: SubscriberClient,
    rooms: Mutex<HashMap<String, Room>>,
}

fn channel(pin: &str) -> String {
    format!("ch:{pin}")
}

impl Hub {
    pub async fn start(redis: Pool, redis_url: &str) -> Result<Arc<Self>> {
        let cfg = fred::types::config::Config::from_url(redis_url)?;
        let sub = Builder::from_config(cfg).build_subscriber_client()?;
        sub.init().await?;
        // Re-subscribes to every channel after a reconnect.
        sub.manage_subscriptions();

        let hub = Arc::new(Hub {
            redis,
            sub,
            rooms: Mutex::new(HashMap::new()),
        });

        let mut rx = hub.sub.message_rx();
        let weak = Arc::downgrade(&hub);
        tokio::spawn(async move {
            while let Ok(msg) = rx.recv().await {
                let Some(hub) = weak.upgrade() else { break };
                let Some(pin) = msg.channel.strip_prefix("ch:") else {
                    continue;
                };
                let Some(raw) = msg.value.as_str() else {
                    continue;
                };
                match serde_json::from_str::<Delivery>(&raw) {
                    Ok(d) => hub.deliver_local(pin, Arc::new(d)),
                    Err(e) => tracing::warn!(error = %e, "bad pub/sub message"),
                }
            }
        });
        Ok(hub)
    }

    fn deliver_local(&self, pin: &str, d: Arc<Delivery>) {
        if let Some(room) = self.rooms.lock().unwrap().get(pin) {
            // No receivers is fine: everyone just left.
            let _ = room.tx.send(d);
        }
    }

    /// Joins this server's room for a game; subscribes on first member.
    pub async fn enter(&self, pin: &str) -> Result<broadcast::Receiver<Arc<Delivery>>> {
        let (rx, first) = {
            let mut rooms = self.rooms.lock().unwrap();
            let room = rooms.entry(pin.to_string()).or_insert_with(|| Room {
                tx: broadcast::channel(ROOM_CAPACITY).0,
                members: 0,
            });
            room.members += 1;
            (room.tx.subscribe(), room.members == 1)
        };
        if first {
            self.sub.subscribe(channel(pin)).await?;
        }
        Ok(rx)
    }

    /// Leaves the room; unsubscribes when the last local socket goes.
    pub async fn leave(&self, pin: &str) {
        let last = {
            let mut rooms = self.rooms.lock().unwrap();
            match rooms.get_mut(pin) {
                Some(room) => {
                    room.members = room.members.saturating_sub(1);
                    if room.members == 0 {
                        rooms.remove(pin);
                        true
                    } else {
                        false
                    }
                }
                None => false,
            }
        };
        if last {
            if let Err(e) = self.sub.unsubscribe(channel(pin)).await {
                tracing::warn!(error = %e, pin, "unsubscribe failed");
            }
        }
    }

    /// Sends to every server with sockets in this game.
    pub async fn publish(&self, pin: &str, d: Delivery) -> Result<()> {
        let raw = serde_json::to_string(&d)?;
        let _: i64 = self.redis.next().publish(channel(pin), raw).await?;
        Ok(())
    }

    pub async fn to_all(&self, pin: &str, push: &ServerPush) -> Result<()> {
        self.publish(pin, Delivery::All { text: encode(push) })
            .await
    }

    pub async fn to_host(&self, pin: &str, push: &ServerPush) -> Result<()> {
        self.publish(pin, Delivery::Host { text: encode(push) })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audience_rules() {
        let t: Arc<str> = "x".into();
        let all = Delivery::All { text: t.clone() };
        assert!(all.text_for(false, Some("p1")).is_some());
        let host = Delivery::Host { text: t.clone() };
        assert!(host.text_for(true, None).is_some());
        assert!(host.text_for(false, Some("p1")).is_none());
        let each = Delivery::Each {
            texts: HashMap::from([("p1".to_string(), t.clone())]),
        };
        assert!(each.text_for(false, Some("p1")).is_some());
        assert!(each.text_for(false, Some("p2")).is_none());
        assert!(each.text_for(true, None).is_none());
        let one = Delivery::Player {
            id: "p2".into(),
            text: t,
        };
        assert!(one.text_for(false, Some("p2")).is_some());
        assert!(one.text_for(false, Some("p1")).is_none());
    }

    #[test]
    fn round_trips_through_json() {
        let d = Delivery::Each {
            texts: HashMap::from([("p1".to_string(), Arc::from("{\"t\":\"x\"}"))]),
        };
        let back: Delivery = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back, d);
    }
}
