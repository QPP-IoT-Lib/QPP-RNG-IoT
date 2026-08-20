//! Shared "which implementations are we comparing" registry.
//!
//! Every `test-harness/*` crate (stats, bench, footprint, differential)
//! needs to iterate "every QPP-RNG variant under test" without hardcoding
//! that list five times over. This crate is that one list.
//!
//! ## Why this exists as its own crate
//!
//! It isn't one of the boxes in the original test-harness diagram -- it's
//! glue the diagram's boxes all need, the same way `xtask` sits outside
//! the diagram but drives it. Putting it in `rng-core` wasn't an option:
//! `rng-core` is unconditionally `#![no_std]` (it has to build for AVR/
//! ESP32 targets), and a registry of `Box<dyn QppRngSource>` factories
//! needs `alloc` at minimum. Every consumer of this crate is host-side
//! tooling anyway, so it's a plain `std` crate.
//!
//! ## Registered today
//!
//! `qpp-rng-reference`'s two raw configurations (XORSHIFT128+ and
//! NEXT_X48 permutation-pad generators), plus each wrapped in
//! [`conditioning::Sha256Conditioner`] -- registering both the raw and
//! conditioned forms side by side is exactly why `conditioning` is a
//! separate crate from `qpp-rng-reference` rather than baked into it
//! (see that crate's module doc): every harness track (stats, bench,
//! footprint, differential) can compare "raw vs. conditioned" for free,
//! with zero special-casing, the same way it compares any other two
//! candidates. `qpp-rng-iot` is still the `cargo new` stub --
//! registering it here would just be a `Candidate` that always returns
//! `4`. Once it has a real `QppRngSource` impl, add it to
//! [`all_candidates`] the same way the entries below are registered;
//! every harness crate picks it up automatically since none of them
//! hardcode candidate names.
//!
//! ## The `Box<dyn QppRngSource>` boundary
//!
//! [`QppRngSource`] (and its `Rng`/`TryRng` supertraits) has no generic
//! methods and no `Self: Sized` bounds, so it's dyn-compatible -- boxing
//! it here is what lets [`Candidate::make`] return "some `QppRngSource`
//! impl, real timer, don't ask which" to callers that just want to
//! iterate every candidate uniformly (stats/bench/footprint).
//!
//! This boxing is also exactly why [`Candidate`] can't help with
//! *mock*-clock determinism testing: [`entropy_timer::HighResTimer`] is a
//! generic type parameter on `QppRng<P, T, N>`, not a trait object, so
//! swapping in a scripted clock needs the concrete generic type, which
//! erasure through `Box<dyn QppRngSource>` has already thrown away.
//! `test-harness/differential` constructs its own generic instances
//! directly against the same two reference configs for that reason --
//! see that crate's `candidates.rs` for the (small, deliberate)
//! duplication this implies.
use std::boxed::Box;

use conditioning::Sha256Conditioner;
use qpp_rng_reference::{QppRngNextX48, QppRngXorshift};
use rng_core::QppRngSource;

/// One named, host-real-timer-backed QPP-RNG configuration under test.
pub struct Candidate {
    /// Stable identifier used as a file/column key throughout the
    /// harness (sample file names, report table columns, criterion
    /// group names, ...). Keep it filesystem- and CSV-safe: lowercase,
    /// `-`-separated, no spaces.
    pub name: &'static str,
    /// Which crate this configuration comes from -- surfaced in reports
    /// so "reference vs iot" stays visible even once several
    /// `qpp-rng-iot` variants exist side by side.
    pub implementation: &'static str,
    /// Permutation-array width `N`, for reports that want it without
    /// constructing an instance first.
    pub array_size: usize,
    /// Builds a fresh instance seeded with `seed`, on the real
    /// [`entropy_timer::PlatformTimer`] for the host this is compiled
    /// on. Every call returns an independent instance.
    pub make: fn(seed: u128) -> Box<dyn QppRngSource>,
}

/// Every QPP-RNG configuration currently under test, real-timer-backed.
///
/// Order is stable and is the order reports render candidates in.
pub fn all_candidates() -> Vec<Candidate> {
    vec![
        Candidate {
            name: "reference-xorshift128plus",
            implementation: "qpp-rng-reference",
            array_size: qpp_rng_reference::DEFAULT_ARRAY_SIZE,
            make: |seed| Box::new(QppRngXorshift::from_seed(seed)),
        },
        Candidate {
            name: "reference-nextx48",
            implementation: "qpp-rng-reference",
            array_size: qpp_rng_reference::DEFAULT_ARRAY_SIZE,
            make: |seed| Box::new(QppRngNextX48::from_seed(seed)),
        },
        Candidate {
            name: "reference-xorshift128plus-sha256-conditioned",
            implementation: "qpp-rng-reference + conditioning",
            array_size: qpp_rng_reference::DEFAULT_ARRAY_SIZE,
            make: |seed| Box::new(Sha256Conditioner::new(QppRngXorshift::from_seed(seed))),
        },
        Candidate {
            name: "reference-nextx48-sha256-conditioned",
            implementation: "qpp-rng-reference + conditioning",
            array_size: qpp_rng_reference::DEFAULT_ARRAY_SIZE,
            make: |seed| Box::new(Sha256Conditioner::new(QppRngNextX48::from_seed(seed))),
        },
        // qpp-rng-iot variants land here once crates/qpp-rng-iot has a
        // real QppRngSource impl. See the module doc above.
    ]
}

/// Looks up one candidate by [`Candidate::name`]. Harness CLIs use this
/// to let a user target a single implementation (`--candidate
/// reference-xorshift128plus`) instead of always running the whole
/// matrix.
pub fn find(name: &str) -> Option<Candidate> {
    all_candidates().into_iter().find(|c| c.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::Rng;

    #[test]
    fn every_candidate_produces_distinct_nonzero_output() {
        for candidate in all_candidates() {
            let mut rng = (candidate.make)(0xC0FF_EE00_1234_5678_9ABC_DEF0_1122_3344);
            let mut buf = [0u8; 64];
            rng.fill_bytes(&mut buf);
            assert!(
                buf.iter().any(|&b| b != buf[0]),
                "candidate {} produced constant output",
                candidate.name
            );
        }
    }

    #[test]
    fn find_looks_up_by_name_and_rejects_unknown_names() {
        assert!(find("reference-xorshift128plus").is_some());
        assert!(find("does-not-exist").is_none());
    }

    #[test]
    fn names_are_unique() {
        let names: Vec<_> = all_candidates().into_iter().map(|c| c.name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names.len(), sorted.len(), "duplicate candidate name");
    }
}
