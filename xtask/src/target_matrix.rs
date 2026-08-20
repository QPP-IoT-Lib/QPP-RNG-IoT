//! Target matrix: host x QEMU x hardware-in-loop, crossed with every
//! implementation crate (`qpp-rng-reference`, `qpp-rng-iot`) -- see
//! `qpp-rng-testing-architecture.md` §5.3, "Cross-target track".
//!
//! ## What's actually validated here
//!
//! Only the `host` rung has been exercised end-to-end in the
//! environment this was built in (see this repo's other
//! `test-harness/*` crates, all verified against it directly). The
//! `qemu` and `hardware-in-loop` entries are real, meaningful
//! definitions -- correct target triples, plausible QEMU
//! machine/`probe-rs` chip identifiers -- but **not** something this
//! session could build or run: none of `qemu-system-*`, `probe-rs`, nor
//! a `xtensa`/`avr`-capable toolchain is installed here, and there's no
//! physical board attached. Treat `qemu_machine`/`probe_rs_chip` values
//! below as a documented starting point to confirm against real
//! hardware/toolchains, not as pre-validated facts.

use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rung {
    Host,
    Qemu,
    HardwareInLoop,
}

impl std::fmt::Display for Rung {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Rung::Host => "host",
            Rung::Qemu => "qemu",
            Rung::HardwareInLoop => "hardware-in-loop",
        })
    }
}

#[derive(Debug, Clone)]
pub struct TargetSpec {
    pub rung: Rung,
    /// Stable identifier, used on the CLI (`--target <name>`) and as a
    /// report/sample-file key component.
    pub name: &'static str,
    /// The `rustc`/Cargo target triple to build for. `None` for `host`
    /// -- omitting `--target` and letting Cargo build for the host
    /// triple is more robust than hardcoding a triple string that has
    /// to match whatever machine this actually runs on.
    pub triple: Option<&'static str>,
    /// `qemu-system-*` machine name, for [`Rung::Qemu`] entries.
    pub qemu_machine: Option<&'static str>,
    /// `probe-rs --chip` identifier, for [`Rung::HardwareInLoop`]
    /// entries.
    pub probe_rs_chip: Option<&'static str>,
    /// Short human-readable description of what real board/machine this
    /// entry corresponds to.
    pub description: &'static str,
}

/// The full cross-target matrix. `entropy_timer`'s own backend `cfg`s
/// (see that crate's `variants/mod.rs`) are the actual source of truth
/// for which triples are meaningful -- every non-host entry here
/// targets one of those backends specifically (`xtensa` for ESP32,
/// `avr` for the Arduino boards, `thumbv7em`/ARMv7-M generically for a
/// Cortex-M QEMU machine the `linux_arm32`/`posix_fallback` backends
/// would cover).
pub fn target_matrix() -> Vec<TargetSpec> {
    vec![
        TargetSpec {
            rung: Rung::Host,
            name: "host",
            triple: None,
            qemu_machine: None,
            probe_rs_chip: None,
            description: "this machine (dev/CI host) -- fastest iteration, full toolchain",
        },
        TargetSpec {
            rung: Rung::Qemu,
            name: "qemu-cortex-m4",
            triple: Some("thumbv7em-none-eabihf"),
            qemu_machine: Some("mps2-an386"),
            probe_rs_chip: None,
            description: "emulated Cortex-M4 (QEMU mps2-an386, a common `cortex-m-quickstart`-style target) -- catches no_std/linking/ABI issues before touching real hardware",
        },
        TargetSpec {
            rung: Rung::HardwareInLoop,
            name: "hil-esp32",
            triple: Some("xtensa-esp32-none-elf"),
            qemu_machine: None,
            probe_rs_chip: Some("esp32"),
            description: "real ESP32 (matches entropy_timer::variants::esp32) -- needs the espup/esp-rs xtensa toolchain, not stable rustc",
        },
        TargetSpec {
            rung: Rung::HardwareInLoop,
            name: "hil-raspi4",
            triple: Some("aarch64-unknown-linux-gnu"),
            qemu_machine: None,
            probe_rs_chip: None,
            description: "real Raspberry Pi 4 over SSH/network, not probe-rs (it's Linux userspace, not a bare-metal probe-rs target) -- matches entropy_timer::variants::linux_arm64",
        },
        TargetSpec {
            rung: Rung::HardwareInLoop,
            name: "hil-arduino-uno",
            triple: Some("avr-unknown-gnu-atmega328"),
            qemu_machine: None,
            probe_rs_chip: Some("ATmega328P"),
            description: "real Arduino Uno (matches entropy_timer::variants::avr) -- needs a nightly `-Z build-std` AVR toolchain",
        },
    ]
}

pub fn find_target(name: &str) -> Option<TargetSpec> {
    target_matrix().into_iter().find(|t| t.name == name)
}

/// The host's own `rustc` target triple, via `rustc -vV`'s `host:`
/// line -- more reliable than guessing from `std::env::consts` (which
/// can't distinguish e.g. `gnu` vs `musl` libc, or ABI variants).
pub fn host_triple() -> anyhow::Result<String> {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run `rustc -vV`: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("`rustc -vV` output had no `host:` line"))
}

/// Checks whether `triple` is in `rustup target list --installed` --
/// i.e. whether attempting to build for it has a chance of working at
/// all, without actually attempting (and failing) the build.
pub fn is_target_installed(triple: &str) -> bool {
    let Ok(output) = Command::new("rustup").args(["target", "list", "--installed"]).output() else {
        // No rustup (e.g. a toolchain installed directly) -- can't
        // check, so don't block the caller on a check that can't run.
        return true;
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.trim() == triple)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_non_host_target_has_a_triple() {
        for t in target_matrix() {
            if t.rung != Rung::Host {
                assert!(t.triple.is_some(), "{} should declare a target triple", t.name);
            }
        }
    }

    #[test]
    fn names_are_unique() {
        let names: Vec<_> = target_matrix().into_iter().map(|t| t.name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names.len(), sorted.len());
    }

    #[test]
    fn find_target_looks_up_by_name() {
        assert!(find_target("host").is_some());
        assert!(find_target("not-a-real-target").is_none());
    }

    #[test]
    fn host_triple_returns_something_plausible() {
        // rustc is guaranteed present in any environment that can build
        // this workspace at all.
        let triple = host_triple().expect("rustc -vV should succeed");
        assert!(triple.contains('-'), "triple {triple:?} looks malformed");
    }
}
