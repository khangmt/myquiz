//! The game state machine (spec section 3.3, FR-GM-05).
//!
//! ```text
//! Lobby ─start─▶ Question(0) ─deadline│all answered│skip─▶ Reveal(0) ─next─▶ Leaderboard(0)
//!                    ▲                                                           │ next
//!                    └──────────────────── Question(i+1) ◀──────────────────────┘
//! Leaderboard(last) ─next─▶ Podium ─next─▶ Ended        (end: any state ─▶ Ended)
//! ```
//!
//! Only the host advances the game, except Question → Reveal, which also
//! fires when the deadline passes or every connected player has answered.
//! Each Redis transition script performs exactly these checks before it
//! writes, so a double click or two workers racing cannot skip a question.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub enum GameState {
    Lobby,
    Question { index: usize },
    Reveal { index: usize },
    Leaderboard { index: usize },
    Podium,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// `host:start`: Lobby → first question.
    Start,
    /// `host:next`: Reveal → Leaderboard → next Question, or last
    /// Leaderboard → Podium → Ended. `expect` is the state the host's screen
    /// showed when they clicked, so a double click is refused, not applied twice.
    Next { expect: GameState },
    /// `host:skip`: end question `index` now.
    Skip { index: usize },
    /// The deadline sweep found question `index` past its end time.
    Deadline { index: usize },
    /// Every connected player has answered question `index`.
    AllAnswered { index: usize },
    /// `host:end`, host timeout or suspension: finish now, keep results.
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransitionError {
    /// The command does not apply in the current state.
    #[error("not allowed in the current state")]
    WrongState,
    /// The command was meant for a state the game has already left
    /// (double click, or another worker won the race). Safe to ignore.
    #[error("the game has already moved on")]
    Stale,
    /// The quiz has no questions.
    #[error("the quiz has no questions")]
    EmptyQuiz,
}

impl GameState {
    /// Applies a command. `total` is the number of questions in the snapshot.
    pub fn apply(self, cmd: Command, total: usize) -> Result<GameState, TransitionError> {
        use Command as C;
        use GameState as S;
        use TransitionError as E;

        match (self, cmd) {
            (S::Ended, _) => Err(E::WrongState),
            (_, C::End) => Ok(S::Ended),

            (S::Lobby, C::Start) if total == 0 => Err(E::EmptyQuiz),
            (S::Lobby, C::Start) => Ok(S::Question { index: 0 }),
            (_, C::Start) => Err(E::Stale),

            (_, C::Next { expect }) if expect != self => Err(E::Stale),
            (S::Reveal { index }, C::Next { .. }) => Ok(S::Leaderboard { index }),
            (S::Leaderboard { index }, C::Next { .. }) if index + 1 < total => {
                Ok(S::Question { index: index + 1 })
            }
            (S::Leaderboard { .. }, C::Next { .. }) => Ok(S::Podium),
            (S::Podium, C::Next { .. }) => Ok(S::Ended),
            (S::Lobby | S::Question { .. }, C::Next { .. }) => Err(E::WrongState),

            (
                S::Question { index },
                C::Skip { index: i } | C::Deadline { index: i } | C::AllAnswered { index: i },
            ) if i == index => Ok(S::Reveal { index }),
            (_, C::Skip { .. } | C::Deadline { .. } | C::AllAnswered { .. }) => Err(E::Stale),
        }
    }

    /// The question this state belongs to, if any.
    pub fn question_index(self) -> Option<usize> {
        match self {
            GameState::Question { index }
            | GameState::Reveal { index }
            | GameState::Leaderboard { index } => Some(index),
            _ => None,
        }
    }

    /// Whether a new player may join (before the lobby lock is considered).
    /// Late joining is allowed until the podium, starting with 0 points.
    pub fn accepts_joins(self) -> bool {
        !matches!(self, GameState::Podium | GameState::Ended)
    }

    pub fn is_finished(self) -> bool {
        matches!(self, GameState::Podium | GameState::Ended)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Command as C;
    use GameState as S;
    use TransitionError as E;

    fn next(s: GameState, total: usize) -> Result<GameState, TransitionError> {
        s.apply(C::Next { expect: s }, total)
    }

    #[test]
    fn full_game_of_two_questions() {
        let mut s = S::Lobby;
        let mut seen = vec![s];
        s = s.apply(C::Start, 2).unwrap();
        seen.push(s);
        s = s.apply(C::Deadline { index: 0 }, 2).unwrap();
        seen.push(s);
        while s != S::Ended {
            s = match s {
                S::Question { index } => s.apply(C::AllAnswered { index }, 2).unwrap(),
                _ => next(s, 2).unwrap(),
            };
            seen.push(s);
        }
        assert_eq!(
            seen,
            vec![
                S::Lobby,
                S::Question { index: 0 },
                S::Reveal { index: 0 },
                S::Leaderboard { index: 0 },
                S::Question { index: 1 },
                S::Reveal { index: 1 },
                S::Leaderboard { index: 1 },
                S::Podium,
                S::Ended,
            ]
        );
    }

    #[test]
    fn cannot_start_an_empty_quiz_or_start_twice() {
        assert_eq!(S::Lobby.apply(C::Start, 0), Err(E::EmptyQuiz));
        assert_eq!(S::Question { index: 0 }.apply(C::Start, 3), Err(E::Stale));
    }

    #[test]
    fn double_click_on_next_does_not_skip() {
        let reveal = S::Reveal { index: 0 };
        let lb = reveal.apply(C::Next { expect: reveal }, 3).unwrap();
        assert_eq!(lb, S::Leaderboard { index: 0 });
        // The second click still carries the old expected state.
        assert_eq!(lb.apply(C::Next { expect: reveal }, 3), Err(E::Stale));
    }

    #[test]
    fn next_is_refused_while_a_question_runs() {
        let q = S::Question { index: 0 };
        assert_eq!(next(q, 3), Err(E::WrongState));
        assert_eq!(next(S::Lobby, 3), Err(E::WrongState));
    }

    #[test]
    fn only_one_reveal_wins() {
        let q = S::Question { index: 2 };
        let revealed = q.apply(C::Deadline { index: 2 }, 5).unwrap();
        assert_eq!(revealed, S::Reveal { index: 2 });
        // A second worker's sweep, or the all-answered trigger, arrives late.
        assert_eq!(revealed.apply(C::Deadline { index: 2 }, 5), Err(E::Stale));
        assert_eq!(
            revealed.apply(C::AllAnswered { index: 2 }, 5),
            Err(E::Stale)
        );
        // A skip for an old question does nothing.
        assert_eq!(
            S::Question { index: 3 }.apply(C::Skip { index: 2 }, 5),
            Err(E::Stale)
        );
    }

    #[test]
    fn end_works_from_anywhere_but_ended() {
        for s in [
            S::Lobby,
            S::Question { index: 0 },
            S::Reveal { index: 0 },
            S::Leaderboard { index: 0 },
            S::Podium,
        ] {
            assert_eq!(s.apply(C::End, 1), Ok(S::Ended));
        }
        assert_eq!(S::Ended.apply(C::End, 1), Err(E::WrongState));
        assert_eq!(next(S::Ended, 1), Err(E::WrongState));
    }

    #[test]
    fn joins_and_helpers() {
        assert!(S::Lobby.accepts_joins());
        assert!(S::Question { index: 1 }.accepts_joins());
        assert!(!S::Podium.accepts_joins());
        assert_eq!(S::Leaderboard { index: 4 }.question_index(), Some(4));
        assert_eq!(S::Lobby.question_index(), None);
    }

    #[test]
    fn serializes_for_redis_and_clients() {
        assert_eq!(
            serde_json::to_string(&S::Lobby).unwrap(),
            r#"{"state":"lobby"}"#
        );
        assert_eq!(
            serde_json::to_string(&S::Question { index: 3 }).unwrap(),
            r#"{"state":"question","index":3}"#
        );
    }
}
