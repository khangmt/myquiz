//! Ranking players (FR-SC-03, FR-SC-04, FR-SC-05).

use serde::{Deserialize, Serialize};

pub const LEADERBOARD_SIZE: usize = 5;
pub const PODIUM_SIZE: usize = 3;

/// What ranking needs to know about a player.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Standing<P> {
    pub player: P,
    pub score: u64,
    /// Sum of response times over answered questions; lower wins a tie.
    pub total_time_ms: u64,
    /// Order of joining the lobby; lower wins a remaining tie.
    pub join_order: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ranked<P> {
    /// 1-based rank. Players with the same score and the same total time
    /// share a rank ("1, 1, 3"); join order only decides who is listed first.
    pub rank: u32,
    #[serde(flatten)]
    pub standing: Standing<P>,
}

/// Sorts by score (high first), then total response time (low first), then
/// join order (low first). Ranks follow score and time only, so two players
/// who are truly level (for example, both on 0 having never answered) share
/// a rank instead of one being ranked lower for joining later. Ranking 200
/// players takes microseconds, so it runs in full after every question.
pub fn rank<P>(mut standings: Vec<Standing<P>>) -> Vec<Ranked<P>> {
    standings.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(a.total_time_ms.cmp(&b.total_time_ms))
            .then(a.join_order.cmp(&b.join_order))
    });
    let mut out: Vec<Ranked<P>> = Vec::with_capacity(standings.len());
    for (i, standing) in standings.into_iter().enumerate() {
        let rank = match out.last() {
            Some(prev)
                if prev.standing.score == standing.score
                    && prev.standing.total_time_ms == standing.total_time_ms =>
            {
                prev.rank
            }
            _ => i as u32 + 1,
        };
        out.push(Ranked { rank, standing });
    }
    out
}

/// The top-5 leaderboard shown after each question.
pub fn leaderboard<P>(ranked: &[Ranked<P>]) -> &[Ranked<P>] {
    &ranked[..ranked.len().min(LEADERBOARD_SIZE)]
}

/// The final top-3 podium.
pub fn podium<P>(ranked: &[Ranked<P>]) -> &[Ranked<P>] {
    &ranked[..ranked.len().min(PODIUM_SIZE)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(
        player: &'static str,
        score: u64,
        total_time_ms: u64,
        join_order: u32,
    ) -> Standing<&'static str> {
        Standing {
            player,
            score,
            total_time_ms,
            join_order,
        }
    }

    fn order(ranked: &[Ranked<&'static str>]) -> Vec<&'static str> {
        ranked.iter().map(|r| r.standing.player).collect()
    }

    #[test]
    fn higher_score_first() {
        let r = rank(vec![
            s("a", 100, 0, 0),
            s("b", 300, 0, 1),
            s("c", 200, 0, 2),
        ]);
        assert_eq!(order(&r), ["b", "c", "a"]);
        assert_eq!(r.iter().map(|x| x.rank).collect::<Vec<_>>(), [1, 2, 3]);
    }

    #[test]
    fn ties_break_on_time_then_join_order() {
        let r = rank(vec![
            s("slow", 500, 9_000, 0),
            s("fast", 500, 3_000, 3),
            s("late_joiner", 500, 3_000, 5),
            s("first_joiner", 500, 3_000, 1),
        ]);
        assert_eq!(order(&r), ["first_joiner", "fast", "late_joiner", "slow"]);
        // Same score and time: same rank, listed by join order.
        assert_eq!(r.iter().map(|x| x.rank).collect::<Vec<_>>(), [1, 1, 1, 4]);
    }

    #[test]
    fn players_level_on_zero_share_a_rank() {
        let r = rank(vec![s("a", 0, 0, 0), s("b", 0, 0, 1), s("c", 10, 500, 2)]);
        assert_eq!(order(&r), ["c", "a", "b"]);
        assert_eq!(r.iter().map(|x| x.rank).collect::<Vec<_>>(), [1, 2, 2]);
    }

    #[test]
    fn leaderboard_and_podium_sizes() {
        let r = rank((0..8).map(|i| s("p", 100 - i as u64, 0, i)).collect());
        assert_eq!(leaderboard(&r).len(), 5);
        assert_eq!(podium(&r).len(), 3);

        let few = rank(vec![s("a", 1, 0, 0), s("b", 2, 0, 1)]);
        assert_eq!(leaderboard(&few).len(), 2);
        assert_eq!(podium(&few).len(), 2);
        assert!(rank::<&str>(vec![]).is_empty());
    }
}
