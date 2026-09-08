//! Picks the right C high-resolution-timer shim for the target being
//! built and compiles it with `cc`. One C file per platform-specific
//! mechanism; the Rust side in `src/variants/` selects the matching FFI
//! wrapper with the exact same predicates used here, so exactly one
//! backend is ever compiled in.
//!
//! Selection order (most to least specific), mirroring the target
//! platform's best available mechanism, falling back to
//! `clock_gettime(CLOCK_MONOTONIC)` on any Unix-like target that isn't
//! one of the specifically-handled ones:
//!
//! | Target                          | Timing source          |
//! |----------------------------------|------------------------|
//! | Windows (any arch)               | `QueryPerformanceCounter` |
//! | macOS, Apple silicon (aarch64)   | `mach_absolute_time()` |
//! | Linux/macOS, x86 or x86_64       | `RDTSC` (invariant-TSC gated, else falls back at runtime) |
//! | Linux, aarch64                   | `CNTVCT_EL0` |
//! | Linux, 32-bit ARM                | PMU cycle counter (falls back to `clock_gettime` at runtime if userspace access isn't granted) |
//! | ESP32 (Xtensa)                   | `CCOUNT` cycle-counter register |
//! | Arduino Uno/Nano (AVR)           | free-running Timer1 |
//! | any other Unix-like target       | `clock_gettime(CLOCK_MONOTONIC)` |

use std::env;

fn main() {
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_family = env::var("CARGO_CFG_TARGET_FAMILY").unwrap_or_default();
    let is_x86 = matches!(target_arch.as_str(), "x86" | "x86_64");

    // Dynamically compile the right C shim for the target.
    let c_file =
        if target_os == "windows" {
            "c/windows_qpc.c"
        } else if target_os == "macos" && target_arch == "aarch64" {
            "c/macos_mach.c"
        } else if (target_os == "linux" || target_os == "macos") && is_x86 {
            "c/x86_tsc.c"
        } else if target_os == "linux" && target_arch == "aarch64" {
            "c/linux_arm64_cntvct.c"
        } else if target_os == "linux" && target_arch == "arm" {
            "c/linux_arm32_pmu.c"
        } else if target_arch == "xtensa" {
            "c/esp32_timer.c"
        } else if target_arch == "avr" {
            "c/avr_timer.c"
        } else if target_family == "unix" {
            "c/posix_timer.c"
        } else {
            panic!(
                "entropy-timer: no high-resolution timer backend for \
             target_arch=\"{target_arch}\", target_os=\"{target_os}\", \
             target_family=\"{target_family}\". Add a c/*.c shim and a \
             matching src/variants/ backend, or build for a supported \
             target."
            );
        };

    println!("cargo:rerun-if-changed={c_file}");

    let mut build = cc::Build::new();
    build.file(c_file);

    // avr-gcc needs -mmcu to select register/interrupt-vector layout.
    // arduino-hal-style build setups export this. Default to the
    // ATmega328P used by both the Uno and the Nano when unset.
    if target_arch == "avr" {
        let mcu = env::var("AVR_MCU").unwrap_or_else(|_| "atmega328p".to_string());
        build.flag(format!("-mmcu={mcu}"));
    }

    build.compile("qpp_entropy_timer");
}
