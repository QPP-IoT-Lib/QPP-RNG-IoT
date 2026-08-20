//! `[[bin]]` "reference-nextx48" -- see `src/lib.rs` for why this
//! exists as its own binary instead of a shared one with a runtime
//! candidate switch.

fn main() {
    qpp_rng_firmware::dump_sample(qpp_rng_reference::QppRngNextX48::from_seed(
        qpp_rng_firmware::SEED,
    ));
}
