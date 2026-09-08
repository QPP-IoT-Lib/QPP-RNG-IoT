// This crate is the IoT/embedded-optimized QPP-RNG variant (see the
// root Cargo.toml's `[profile.release.package.qpp-rng-iot]` flash-size
// override) -- unlike `qpp-rng-reference`, which also runs on host for
// testing/benchmarking, this one only ever targets no_std hardware, so
// it stays `no_std` unconditionally rather than gating on target_os.
#![no_std]

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
