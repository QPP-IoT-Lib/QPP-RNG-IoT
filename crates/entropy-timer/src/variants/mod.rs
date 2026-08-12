//! Platform-specific high-resolution timer backends.
//!
//! Each backend is a thin, safe wrapper around a C shim (compiled by
//! `build.rs` via the `cc` crate) that reads the cheapest, highest-
//! resolution free-running counter the target actually has, with a
//! runtime fallback to `clock_gettime(CLOCK_MONOTONIC)` wherever the
//! preferred mechanism isn't reliably available on every CPU that could
//! be running the target OS.
//!
//! `build.rs` selects exactly one C file per build using the same
//! predicates as the `cfg`s below, so exactly one of these modules is
//! ever compiled in, and [`PlatformTimer`] always names the right one
//! for the target being built.

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::WindowsTimer as PlatformTimer;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod macos_arm;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use macos_arm::MacosArmTimer as PlatformTimer;

#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "x86", target_arch = "x86_64")
))]
mod x86_tsc;
#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "x86", target_arch = "x86_64")
))]
pub use x86_tsc::X86TscTimer as PlatformTimer;

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
mod linux_arm64;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub use linux_arm64::LinuxArm64Timer as PlatformTimer;

#[cfg(all(target_os = "linux", target_arch = "arm"))]
mod linux_arm32;
#[cfg(all(target_os = "linux", target_arch = "arm"))]
pub use linux_arm32::LinuxArm32Timer as PlatformTimer;

#[cfg(target_arch = "xtensa")]
mod esp32;
#[cfg(target_arch = "xtensa")]
pub use esp32::Esp32Timer as PlatformTimer;

#[cfg(target_arch = "avr")]
mod avr;
#[cfg(target_arch = "avr")]
pub use avr::AvrTimer as PlatformTimer;

// Generic Unix-like fallback: anything covered by none of the more
// specific backends above (e.g. Linux on riscv64/mips/powerpc, a BSD).
#[cfg(all(
    unix,
    not(all(target_os = "macos", target_arch = "aarch64")),
    not(all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "x86", target_arch = "x86_64")
    )),
    not(all(target_os = "linux", target_arch = "aarch64")),
    not(all(target_os = "linux", target_arch = "arm")),
))]
mod posix_fallback;
#[cfg(all(
    unix,
    not(all(target_os = "macos", target_arch = "aarch64")),
    not(all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "x86", target_arch = "x86_64")
    )),
    not(all(target_os = "linux", target_arch = "aarch64")),
    not(all(target_os = "linux", target_arch = "arm")),
))]
pub use posix_fallback::PosixFallbackTimer as PlatformTimer;

/// A high-resolution, free-running tick source backed by the cheapest
/// hardware counter available on the current target.
///
/// The returned value varies in resolution and units per platform
/// (nanoseconds on most host OSes, raw CPU/timer cycles on RDTSC/PMU/
/// CCOUNT/Timer1 paths) and is **not** a calibrated wall-clock
/// timestamp. Callers only ever need the *difference* between two
/// consecutive ticks (jitter), computed with wrapping arithmetic to
/// stay correct across the counter's own wraparound.
pub trait HighResTimer {
    /// One-time hardware setup. No-op on platforms where the counter is
    /// already free-running from reset/boot (host OSes, ESP32);
    /// required on AVR, which has to configure Timer1 first.
    fn init(&mut self);

    /// Cheapest possible read of the underlying hardware counter.
    fn tick(&mut self) -> u64;
}
