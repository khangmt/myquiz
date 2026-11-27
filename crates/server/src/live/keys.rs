//! Redis key names for live games (design section 4). Every key of a game
//! expires 24 h after the game ends.

pub const DEADLINES: &str = "games:deadlines";

pub fn pin(pin: &str) -> String {
    format!("pin:{pin}")
}
pub fn game(pin: &str) -> String {
    format!("game:{pin}")
}
pub fn quiz(pin: &str) -> String {
    format!("game:{pin}:quiz")
}
pub fn players(pin: &str) -> String {
    format!("game:{pin}:players")
}
pub fn tokens(pin: &str) -> String {
    format!("game:{pin}:tokens")
}
pub fn nicks(pin: &str) -> String {
    format!("game:{pin}:nicks")
}
pub fn kicked(pin: &str) -> String {
    format!("game:{pin}:kicked")
}
pub fn answers(pin: &str, index: usize) -> String {
    format!("game:{pin}:ans:{index}")
}
/// Player id to number of open sockets on any server.
pub fn online(pin: &str) -> String {
    format!("game:{pin}:online")
}
/// Login session written by the auth module (design section 9, SEC-04).
pub fn login_session(sid: &str) -> String {
    format!("session:{sid}")
}
/// A player's chat cooldown, per kind ("text" or "emoji").
pub fn chat_cooldown(pin: &str, player_id: &str, kind: &str) -> String {
    format!("game:{pin}:chat:{player_id}:{kind}")
}
/// Chat messages in the current second, for the whole game.
pub fn chat_rate(pin: &str) -> String {
    format!("game:{pin}:chat:rate")
}
