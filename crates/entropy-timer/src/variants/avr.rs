//! Backend for the Arduino Uno / Arduino Nano (ATmega328P, AVR). Wraps
//! the free-running Timer1 in `c/avr_timer.c`.

use super::HighResTimer;

unsafe extern "C" {
    fn qpp_timer_init();
    fn qpp_timer_tick() -> u64;
}

/// Timer1 driven with no prescaler (ticks once per CPU clock cycle),
/// extended past its native 16-bit width to 32 bits via its overflow
/// interrupt. [`init`](HighResTimer::init) must run before the first
/// [`tick`](HighResTimer::tick) — it starts the timer and enables that
/// interrupt.
#[derive(Default)]
pub struct AvrTimer;

impl HighResTimer for AvrTimer {
    fn init(&mut self) {
        // SAFETY: `qpp_timer_init` takes no arguments; it configures
        // Timer1 and enables its overflow interrupt, which is safe to
        // do at any point before interrupts are relied upon elsewhere.
        unsafe { qpp_timer_init() }
    }

    fn tick(&mut self) -> u64 {
        // SAFETY: `qpp_timer_tick` takes no arguments and only reads
        // hardware/ISR-maintained state behind its own interrupt guard.
        unsafe { qpp_timer_tick() }
    }
}
