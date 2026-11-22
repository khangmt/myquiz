//! Pure game logic: no I/O, no clock, no randomness.
//!
//! Everything here takes its inputs as arguments so it can be unit tested in
//! isolation. The live game gateway calls into this crate; Redis scripts hold
//! the state, this crate decides what is allowed and what it scores.
//!
//! The state machine, scoring and answer matching land in the next PR.
