//! Backend for Windows (any CPU arch). Wraps the
//! `QueryPerformanceCounter` shim in `c/windows_qpc.c`.

use super::HighResTimer;

unsafe extern "C" {
    fn qpp_timer_tick() -> u64;
}

/// OS-arbitrated high-resolution counter; monotonic and consistent
/// across cores by construction, so it needs no runtime fallback.
#[derive(Default)]
pub struct WindowsTimer;

impl HighResTimer for WindowsTimer {
    fn init(&mut self) -> u8 {
        100 // ns is the native timer resolution on Windows
    }

    fn tick(&mut self) -> u64 {
        // SAFETY: `qpp_timer_tick` takes no arguments, has no
        // preconditions, and only reads OS-owned clock state.
        unsafe { qpp_timer_tick() }
    }
}
