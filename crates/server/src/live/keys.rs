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
