//! `[[bin]]` "reference-xorshift128plus" -- see `src/lib.rs` for why
//! this exists as its own binary instead of a shared one with a
//! runtime candidate switch.

fn main() {
    qpp_rng_firmware::dump_sample(qpp_rng_reference::QppRngXorshift::from_seed(
        qpp_rng_firmware::SEED,
    ));
}
