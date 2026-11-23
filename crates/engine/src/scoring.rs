//! Points for one answer (FR-SC-01). Computed only on the server.

/// `round(P × (1 − t / (2T)))` for a correct answer, 0 otherwise.
///
/// `response_ms` is clamped to `[0, time_limit_ms]`, so an instant correct
/// answer earns `P` and one at the deadline earns `P / 2`. Rounds half away
/// from zero, the usual meaning of "round".
pub fn points(correct: bool, base_points: u32, response_ms: u64, time_limit_ms: u64) -> u32 {
    if !correct || base_points == 0 || time_limit_ms == 0 {
        return 0;
    }
    let t = response_ms.min(time_limit_ms);
    // Integer arithmetic avoids float edge cases:
    // P × (2T − t) / 2T, rounded half up.
    let p = u128::from(base_points);
    let num = p * u128::from(2 * time_limit_ms - t);
    let den = u128::from(2 * time_limit_ms);
    ((num * 2 + den) / (den * 2)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_or_zero_point_questions_score_nothing() {
        assert_eq!(points(false, 1000, 0, 20_000), 0);
        assert_eq!(points(true, 0, 0, 20_000), 0);
    }

    #[test]
    fn instant_answer_gets_full_points() {
        assert_eq!(points(true, 1000, 0, 20_000), 1000);
        assert_eq!(points(true, 2000, 0, 20_000), 2000);
    }

    #[test]
    fn answer_at_the_deadline_gets_half() {
        assert_eq!(points(true, 1000, 20_000, 20_000), 500);
        assert_eq!(points(true, 2000, 20_000, 20_000), 1000);
    }

    #[test]
    fn late_time_is_clamped() {
        assert_eq!(points(true, 1000, 25_000, 20_000), 500);
    }

    #[test]
    fn halfway_and_rounding() {
        assert_eq!(points(true, 1000, 10_000, 20_000), 750);
        // 1000 × (1 − 1234 / 40000) = 969.15 → 969
        assert_eq!(points(true, 1000, 1234, 20_000), 969);
        // 1000 × (1 − 3 / 40) = 925 exactly
        assert_eq!(points(true, 1000, 3_000, 20_000), 925);
        // 1000 × (1 − 1 / 16) = 937.5 → 938 (half rounds up)
        assert_eq!(points(true, 1000, 1_250, 10_000), 938);
    }

    #[test]
    fn matches_the_formula_everywhere() {
        // P × (2T − t) is an exact integer, so dividing once in f64 gives the
        // exact value of P × (1 − t/2T) wherever it is a half (x.5). Writing
        // the formula literally in floats can land on 1660.4999… instead of
        // 1660.5 and round the wrong way, which is why `points` uses integers.
        for &limit_s in &[5u64, 20, 30, 240] {
            let tt = limit_s * 1000;
            for t in (0..=tt).step_by(97) {
                for &p in &[1000u32, 2000] {
                    let expected = (f64::from(p) * (2 * tt - t) as f64 / (2 * tt) as f64).round();
                    assert_eq!(
                        f64::from(points(true, p, t, tt)),
                        expected,
                        "p={p} t={t} T={tt}"
                    );
                }
            }
        }
        // The case that motivated integers: 2000 × (1 − 6790/40000) = 1660.5.
        assert_eq!(points(true, 2000, 6_790, 20_000), 1661);
    }

    #[test]
    fn faster_never_scores_less() {
        let mut last = u32::MAX;
        for t in (0..=20_000).step_by(50) {
            let p = points(true, 1000, t, 20_000);
            assert!(p <= last);
            last = p;
        }
    }
}
