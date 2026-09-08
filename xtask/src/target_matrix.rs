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
//!
//! The AVR entries' `triple`/`nightly`/`build_std`/`rustflags` fields
//! *have* been confirmed directly, though (just the plain `cargo build`
//! invocation on a Rust-only crate, not linking against real avr-gcc
//! output -- see each entry's own comment): current rustc/LLVM only
//! ships one generic Tier 3 `avr-none` target (`need-explicit-cpu` in
//! its target spec) with no prebuilt `core`, not the older per-chip
//! `avr-unknown-gnu-atmegaXXX` triples that used to exist, so hitting a
//! specific MCU means `-Z build-std=core --target avr-none` plus
//! `RUSTFLAGS=-C target-cpu=<mcu>` rather than a distinct `--target`.

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
    /// Extra `key=value` environment variables to set for the
    /// `cargo build` invocation targeting this entry -- e.g. AVR's
    /// `AVR_MCU`, which `entropy-timer`'s build.rs reads to pick the
    /// right `-mmcu` flag for chips other than its ATmega328P default.
    /// Empty for entries that don't need any.
    pub env: &'static [(&'static str, &'static str)],
    /// Whether this target needs the nightly toolchain (currently just
    /// AVR, for `-Z build-std`; every prebuilt-std target -- the Linux
    /// SBC triples and `thumbv7em-none-eabihf` -- builds on stable).
    pub nightly: bool,
    /// `-Z build-std=<crates>` value, for targets with no prebuilt
    /// `std`/`core` component to download (currently just `avr-none`,
    /// a Tier 3 target -- see this module's doc comment). `None` means
    /// the target ships a prebuilt component, so plain `--target` is
    /// enough.
    pub build_std: Option<&'static str>,
    /// Extra `rustc` flags, applied via `RUSTFLAGS`. AVR's `avr-none`
    /// is a single generic target covering every AVR chip
    /// (`need-explicit-cpu` in its target spec), so hitting a specific
    /// MCU means passing e.g. `-C target-cpu=atmega328p` here --
    /// there's no separate per-chip target triple to select it with.
    pub rustflags: &'static [&'static str],
}

/// The full cross-target matrix. `entropy_timer`'s own backend `cfg`s
/// (see that crate's `variants/mod.rs`) are the actual source of truth
/// for which triples are meaningful -- every non-host entry here
/// targets one of those backends specifically (`xtensa` for ESP32,
/// `avr` for the Arduino boards -- Uno, Nano, and Mega 2560 --, `arm`/
/// `linux` for the Raspberry Pi 0 and 4, and bare-metal `thumbv7em` for
/// Cortex-M boards such as the makerdiary nRF52840 MDK).
pub fn target_matrix() -> Vec<TargetSpec> {
    vec![
        TargetSpec {
            rung: Rung::Host,
            name: "host",
            triple: None,
            qemu_machine: None,
            probe_rs_chip: None,
            description: "this machine (dev/CI host) -- fastest iteration, full toolchain",
            env: &[],
            nightly: false,
            build_std: None,
            rustflags: &[],
        },
        TargetSpec {
            rung: Rung::Qemu,
            name: "qemu-cortex-m4",
            triple: Some("thumbv7em-none-eabihf"),
            qemu_machine: Some("mps2-an386"),
            probe_rs_chip: None,
            description: "emulated Cortex-M4 (QEMU mps2-an386, a common `cortex-m-quickstart`-style target) -- catches no_std/linking/ABI issues before touching real hardware; also matches entropy_timer::variants::cortex_m, though the QEMU machine model may not implement the DWT unit that backend reads",
            env: &[],
            nightly: false,
            build_std: None,
            rustflags: &[],
        },
        TargetSpec {
            rung: Rung::HardwareInLoop,
            name: "hil-esp32",
            triple: Some("xtensa-esp32-none-elf"),
            qemu_machine: None,
            probe_rs_chip: Some("esp32"),
            description: "real ESP32 (matches entropy_timer::variants::esp32) -- needs the espup/esp-rs xtensa toolchain, not stable rustc",
            env: &[],
            nightly: false,
            build_std: None,
            rustflags: &[],
        },
        TargetSpec {
            rung: Rung::HardwareInLoop,
            name: "hil-raspi4",
            triple: Some("aarch64-unknown-linux-gnu"),
            qemu_machine: None,
            probe_rs_chip: None,
            description: "real Raspberry Pi 4 (64-bit Raspberry Pi OS) over SSH/network, not probe-rs (it's Linux userspace, not a bare-metal probe-rs target) -- matches entropy_timer::variants::linux_arm64. Running a 32-bit Raspberry Pi OS instead means this is really `hil-raspi0`'s armv7 triple/backend, not this one",
            env: &[],
            nightly: false,
            build_std: None,
            rustflags: &[],
        },
        TargetSpec {
            rung: Rung::HardwareInLoop,
            name: "hil-raspi0",
            triple: Some("arm-unknown-linux-gnueabihf"),
            qemu_machine: None,
            probe_rs_chip: None,
            description: "real Raspberry Pi 0 (ARM1176JZF-S, ARMv6) over SSH/network, same as hil-raspi4 -- matches entropy_timer::variants::linux_arm32, but this core has no PMU generation that shim's cycle-counter probe recognizes, so it always falls back to clock_gettime(CLOCK_MONOTONIC) rather than raw cycles",
            env: &[],
            nightly: false,
            build_std: None,
            rustflags: &[],
        },
        TargetSpec {
            rung: Rung::HardwareInLoop,
            name: "hil-arduino-uno",
            // Current rustc/LLVM ships one generic Tier 3 `avr-none`
            // target, not the older per-chip `avr-unknown-gnu-atmegaXXX`
            // triples -- confirmed directly against this toolchain (see
            // this module's doc comment). Hitting the ATmega328P means
            // `-Z build-std=core` plus `-C target-cpu=atmega328p` in
            // `rustflags` below, since `avr-none` covers every AVR chip.
            triple: Some("avr-none"),
            qemu_machine: None,
            // Not `probe_rs_chip: Some("ATmega328P")`, on purpose: classic
            // AVR parts like this one don't expose SWD/JTAG, so `probe-rs`'s
            // flash+RTT model (crate::hil::ProbeRsFlasher/RttTelemetry)
            // doesn't apply here at all. Flashing goes through `avrdude`
            // (what `ravedude`, the usual `avr-hal` runner, wraps) over the
            // board's ISP/bootloader instead, and telemetry has to be
            // `crate::hil::UartTelemetry` over its USB-serial bridge --
            // there is no RTT-equivalent path on this chip.
            probe_rs_chip: None,
            description: "real Arduino Uno (ATmega328P, matches entropy_timer::variants::avr with its default -mmcu) -- needs a nightly `-Z build-std` AVR toolchain and avr-gcc; flash via avrdude/ravedude, not probe-rs, and read samples back over UART",
            env: &[],
            nightly: true,
            build_std: Some("core"),
            rustflags: &["-C", "target-cpu=atmega328p"],
        },
        TargetSpec {
            rung: Rung::HardwareInLoop,
            name: "hil-arduino-nano",
            triple: Some("avr-none"),
            qemu_machine: None,
            // Same MCU/triple/probe-rs situation as the Uno above (also
            // an ATmega328P with no SWD/JTAG) -- the only real difference
            // is the board's USB-serial bridge and, on older classic
            // Nanos, avrdude needing `-c arduino -b 57600` instead of the
            // Uno's 115200 for the bootloader.
            probe_rs_chip: None,
            description: "real Arduino Nano (ATmega328P, matches entropy_timer::variants::avr) -- same toolchain/flashing story as hil-arduino-uno; older classic Nanos need a slower avrdude bootloader baud rate (57600 vs. 115200)",
            env: &[],
            nightly: true,
            build_std: Some("core"),
            rustflags: &["-C", "target-cpu=atmega328p"],
        },
        TargetSpec {
            rung: Rung::HardwareInLoop,
            name: "hil-arduino-mega2560",
            triple: Some("avr-none"),
            qemu_machine: None,
            probe_rs_chip: None,
            description: "real Arduino Mega 2560 (ATmega2560, matches entropy_timer::variants::avr, whose Timer1 register names avr-libc defines identically to the ATmega328P) -- needs AVR_MCU=atmega2560 so entropy-timer's build.rs passes the right -mmcu to avr-gcc for the C shim, and -C target-cpu=atmega2560 (below) so rustc/LLVM codegen the right instruction subset/memory layout for the Rust side; same avrdude/UART flashing story as the Uno/Nano",
            env: &[("AVR_MCU", "atmega2560")],
            nightly: true,
            build_std: Some("core"),
            rustflags: &["-C", "target-cpu=atmega2560"],
        },
        TargetSpec {
            rung: Rung::HardwareInLoop,
            name: "hil-makerdiary-nrf52840",
            triple: Some("thumbv7em-none-eabihf"),
            qemu_machine: None,
            probe_rs_chip: Some("nRF52840_xxAA"),
            description: "real makerdiary nRF52840 MDK (Nordic nRF52840, Cortex-M4F, matches entropy_timer::variants::cortex_m's DWT->CYCCNT backend) -- the board's onboard DAPLink exposes SWD directly, so this flashes/runs through crate::hil::ProbeRsFlasher like hil-esp32, no avrdude/UART step needed. Unlike AVR this is a prebuilt-std Tier 2 target, so plain stable rustc + arm-none-eabi-gcc is enough, no nightly/build-std",
            env: &[],
            nightly: false,
            build_std: None,
            rustflags: &[],
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
