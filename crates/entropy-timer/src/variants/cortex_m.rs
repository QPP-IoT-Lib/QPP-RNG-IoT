//! Backend for bare-metal Cortex-M3/M4/M7 targets with a DWT unit,
//! such as the Nordic nRF52840 (e.g. the makerdiary nRF52840 MDK).
//! Wraps the free-running DWT cycle counter in `c/cortex_m_dwt.c`.

use super::HighResTimer;

unsafe extern "C" {
    fn qpp_timer_init();
    fn qpp_timer_tick() -> u64;
}

/// DWT->CYCCNT driven with no prescaler (ticks once per CPU clock
/// cycle). [`init`](HighResTimer::init) must run before the first
/// [`tick`](HighResTimer::tick) -- it enables the trace subsystem and
/// starts the counter.
#[derive(Default)]
pub struct CortexMTimer;

impl HighResTimer for CortexMTimer {
    fn init(&mut self) -> u8 {
        // SAFETY: `qpp_timer_init` takes no arguments; it only touches
        // the DWT/DEMCR debug peripherals (memory-mapped, always
        // accessible to code already running on the core), which is
        // safe to enable at any point before timing is relied upon.
        unsafe { qpp_timer_init() }
        // ns, nominal for a 64MHz core clock (the nRF52840's HFCLK) --
        // `calibrate_resolution` re-measures the real value at runtime,
        // this is only the fallback if that measurement can't proceed.
        16
    }

    fn tick(&mut self) -> u64 {
        // SAFETY: `qpp_timer_tick` takes no arguments and only reads
        // the free-running DWT->CYCCNT register.
        unsafe { qpp_timer_tick() }
    }
}
