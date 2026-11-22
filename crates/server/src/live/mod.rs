//! Live game state in Redis (design section 4).
//!
//! Game servers hold no game state of their own. Every game lives in Redis
//! and every change is one atomic Lua script, so any worker can serve any
//! player and a worker restart loses nothing (NFR-06, NFR-08). The rules
//! (what state comes next, what is correct, what it scores) come from
//! `myquiz_engine`; the scripts only make each step atomic.

pub mod keys;
pub mod secrets;
mod store;

pub use store::*;

#[cfg(test)]
mod tests;
