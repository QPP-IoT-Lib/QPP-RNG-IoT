//! `[[bin]]` "reference-nextx48-sha256-conditioned" -- see `src/lib.rs`
//! for why this exists as its own binary instead of a shared one with a
//! runtime candidate switch.

fn main() {
    qpp_rng_firmware::dump_sample(conditioning::Sha256Conditioner::new(
        qpp_rng_reference::QppRngNextX48::from_seed(qpp_rng_firmware::SEED),
    ));
}
