//! A scripted [`HighResTimer`] for pinning exact jitter sequences in
//! tests -- the same technique `qpp-rng-reference`'s own test suite
//! uses (`QppRng`'s private `MockTimer`), pulled out here as a shared,
//! reusable, public type so `differential`'s determinism/fuzz checks
//! don't have to reimplement it.

use entropy_timer::HighResTimer;

/// Replays a fixed sequence of per-cycle deltas as consecutive
/// [`HighResTimer::tick`] calls: every *pair* of calls (the start/stop
/// of one `QppRng` convergence cycle) differs by exactly the next
/// scripted delta, regardless of how many other `tick()`-unrelated work
/// happens between them. The sequence repeats (`% deltas.len()`) if
/// more cycles run than deltas were provided.
#[derive(Debug, Clone)]
pub struct MockClock {
    deltas: Vec<u64>,
    idx: usize,
    cumulative: u64,
}

impl MockClock {
    /// # Panics
    /// If `deltas` is empty -- a clock that never advances by anything
    /// isn't a meaningful script to replay.
    pub fn new(deltas: Vec<u64>) -> Self {
        assert!(!deltas.is_empty(), "MockClock needs at least one scripted delta");
        Self {
            deltas,
            idx: 0,
            cumulative: 0,
        }
    }
}

impl HighResTimer for MockClock {
    fn init(&mut self) -> u8 {
        1
    }

    fn tick(&mut self) -> u64 {
        if self.idx % 2 == 1 {
            let d = self.deltas[(self.idx / 2) % self.deltas.len()];
            self.cumulative = self.cumulative.wrapping_add(d);
        }
        let value = self.cumulative;
        self.idx += 1;
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consecutive_tick_pairs_differ_by_the_scripted_delta() {
        let mut clock = MockClock::new(vec![7, 13, 5]);
        let a0 = clock.tick();
        let a1 = clock.tick();
        assert_eq!(a1.wrapping_sub(a0), 7);
        let b0 = clock.tick();
        let b1 = clock.tick();
        assert_eq!(b1.wrapping_sub(b0), 13);
    }

    #[test]
    fn script_repeats_once_exhausted() {
        let mut clock = MockClock::new(vec![10]);
        for _ in 0..3 {
            let a = clock.tick();
            let b = clock.tick();
            assert_eq!(b.wrapping_sub(a), 10);
        }
    }

    #[test]
    #[should_panic(expected = "at least one")]
    fn rejects_empty_script() {
        let _ = MockClock::new(vec![]);
    }
}
