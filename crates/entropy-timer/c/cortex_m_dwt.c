/* High-resolution timer backend for bare-metal Cortex-M3/M4/M7 targets
 * that implement the DWT (Data Watchpoint and Trace) unit -- e.g. the
 * Nordic nRF52840 (Cortex-M4F) on the makerdiary nRF52840 MDK.
 *
 * DWT->CYCCNT is a free-running 32-bit cycle counter, ticking once per
 * CPU clock cycle -- the finest grain this core exposes. It's disabled
 * out of reset and gated behind two enables:
 *
 *   1. DEMCR.TRCENA (the debug exception/monitor control register's
 *      trace-enable bit) has to be set first; it gates the whole debug/
 *      trace subsystem the DWT belongs to.
 *   2. DWT->CTRL.CYCCNTENA then starts the counter itself.
 *
 * Unlike the Linux ARM32 PMU shim, there's no userspace/kernel
 * privilege split to probe against here: bare-metal code already runs
 * with full access to the debug peripherals memory-mapped at
 * 0xE0000000, so enabling the counter can't trap. If a future Cortex-M
 * target turns out not to implement DWT (e.g. Cortex-M0/M0+, which
 * have no cycle counter at all), it needs its own backend rather than
 * reusing this one.
 */

#include <stdint.h>

#define QPP_DEMCR ((volatile uint32_t *)0xE000EDFCu)
#define QPP_DWT_CTRL ((volatile uint32_t *)0xE0001000u)
#define QPP_DWT_CYCCNT ((volatile uint32_t *)0xE0001004u)

#define QPP_DEMCR_TRCENA (1u << 24)
#define QPP_DWT_CTRL_CYCCNTENA (1u << 0)

void qpp_timer_init(void) {
    *QPP_DEMCR |= QPP_DEMCR_TRCENA;
    *QPP_DWT_CYCCNT = 0;
    *QPP_DWT_CTRL |= QPP_DWT_CTRL_CYCCNTENA;
}

uint64_t qpp_timer_tick(void) {
    return (uint64_t)*QPP_DWT_CYCCNT;
}
