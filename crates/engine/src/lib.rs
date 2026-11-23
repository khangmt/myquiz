//! Pure game logic: no I/O, no clock, no randomness.
//!
//! Everything here takes its inputs as arguments so it can be unit tested in
//! isolation. Redis holds the live state and the gateway does the I/O; this
//! crate decides what is allowed, what is correct and what it scores.
//!
//! | Module      | Covers |
//! | ----------- | ------ |
//! | [`quiz`]    | The quiz snapshot a game runs on, and the publish rules (FR-QZ-02..06, FR-QZ-10) |
//! | [`state`]   | Lobby → Question → Reveal → Leaderboard → … → Podium → Ended (spec 3.3, FR-GM-05) |
//! | [`answer`]  | Checking a submission's shape and timing (FR-GM-06, SEC-16) |
//! | [`matching`]| Is an answer correct? Exact set for multi, normalised text for fill-in (FR-QZ-02, FR-QZ-04) |
//! | [`scoring`] | `round(P × (1 − t / 2T))` (FR-SC-01) |
//! | [`ranking`] | Ranks with tie-breaks, top 5, podium (FR-SC-03..05) |
//! | [`reveal`]  | Scores a whole question and builds the answer distribution (FR-GM-07, FR-GM-08) |
//! | [`view`]    | What players may see of a question, built from an allow-list (SEC-15) |

pub mod answer;
pub mod matching;
pub mod profanity;
pub mod quiz;
pub mod ranking;
pub mod reveal;
pub mod scoring;
pub mod state;
pub mod view;

pub use answer::{Submission, SubmitError};
pub use quiz::{OptionId, Question, QuestionKind, QuizSnapshot};
pub use state::{Command, GameState, TransitionError};
