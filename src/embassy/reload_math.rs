//! Pure rearm arithmetic shared by reload-on-compare timer backends.

// Leave a few dozen clocks for the count-read / end-write bus accesses.
const MIN_PERIOD: u32 = 64;

pub(super) fn compare_armed(count_after_write: u32, period: u32) -> bool {
    // END is period-1. Equality is a valid match, not a missed compare.
    count_after_write < period
}

pub(super) fn rearm(base: u64, count: u32, at: u64, max_period: u32, mask: u32, resets: bool) -> (u64, u32) {
    if resets {
        let base = base + count as u64;
        let period = at.saturating_sub(base).min(max_period as u64) as u32;
        (base, period.max(MIN_PERIOD))
    } else {
        let period = at.saturating_sub(base).min(max_period as u64) as u32;
        (base, period.max(count.saturating_add(MIN_PERIOD)).min(mask))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_compare_write_requires_retry() {
        for period in [64, 1000, 0x03ffffff, 0x7fffffff] {
            assert!(compare_armed(period - 2, period));
            assert!(compare_armed(period - 1, period));
            assert!(!compare_armed(period, period));
            assert!(!compare_armed(period + 1, period));
        }
    }
    #[test]
    fn preserve_elapsed_when_write_resets_count() {
        let (base, period) = rearm(1000, 100, 1600, 10000, 20000, true);
        assert_eq!((base, period), (1100, 500));
        assert_eq!(base + period as u64, 1600);
        // Repeated reprogramming must never charge elapsed time twice.
        assert_eq!(rearm(base, 50, 1600, 10000, 20000, true), (1150, 450));
    }
    #[test]
    fn compare_only_backend_preserves_origin() {
        assert_eq!(rearm(1000, 100, 1600, 10000, 20000, false), (1000, 600));
        assert_eq!(rearm(1000, 100, 1050, 10000, 20000, false), (1000, 164));
    }
    #[test]
    fn past_deadlines_and_long_waits() {
        for resets in [false, true] {
            let (base, period) = rearm(1000, 100, 0, 10000, 20000, resets);
            assert_eq!(base + period as u64, 1164);
            assert_eq!(rearm(1000, 0, u64::MAX, 10000, 20000, resets).1, 10000);
        }
    }
}
