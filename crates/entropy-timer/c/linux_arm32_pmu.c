/* High-resolution timer backend for 32-bit ARM (ARMv7-A) Linux, e.g. a
 * 32-bit kernel on Raspberry Pi 4.
 *
 * PMCCNTR (the PMU's free-running cycle counter, accessed via
 * `mrc p15, 0, Rd, c9, c13, 0`) ticks once per CPU cycle -- the finest
 * grain this core exposes -- but unlike aarch64's CNTVCT_EL0, PL0
 * (userspace) access to the whole PMU register block is gated by
 * PMUSERENR.EN, a bit only PL1 (the kernel) can set. Nothing in
 * userspace can grant that access itself; it has to already be enabled
 * by the running kernel (e.g. via an `enable_arm_pmu`-style module or
 * board config), which is not the default on Raspberry Pi OS. Trying to
 * touch any PMU register without that access raises SIGILL.
 *
 * So this shim probes once, at first use: install a SIGILL handler,
 * attempt to enable and read the cycle counter, and use sigsetjmp to
 * bail out cleanly if it traps. The result is cached for the life of
 * the process -- when the probe fails, every subsequent call falls
 * back to clock_gettime(CLOCK_MONOTONIC) instead.
 *
 * Raspberry Pi 0 (ARM1176, ARMv6) doesn't implement this PMU generation
 * at all, so it always takes the fallback path here too.
 */

#include <setjmp.h>
#include <signal.h>
#include <stdint.h>
#include <time.h>

static int pmu_checked = 0;
static int pmu_available = 0;
static sigjmp_buf pmu_probe_env;

static void pmu_sigill_handler(int sig) {
    (void)sig;
    siglongjmp(pmu_probe_env, 1);
}

static uint32_t read_pmccntr(void) {
    uint32_t val;
    __asm__ __volatile__("mrc p15, 0, %0, c9, c13, 0" : "=r"(val));
    return val;
}

/* Enables the cycle counter (PMCR.E, PMCNTENSET bit 31). Only reachable
 * from PL0 at all if the kernel has already set PMUSERENR.EN -- if not,
 * the first instruction here traps into the SIGILL handler above. */
static void enable_cycle_counter(void) {
    uint32_t pmcr;
    __asm__ __volatile__("mrc p15, 0, %0, c9, c12, 0" : "=r"(pmcr));
    pmcr |= (1u << 0) | (1u << 2); /* E: enable; C: reset cycle counter */
    __asm__ __volatile__("mcr p15, 0, %0, c9, c12, 0" : : "r"(pmcr));

    uint32_t cntens = (1u << 31); /* enable the cycle counter itself */
    __asm__ __volatile__("mcr p15, 0, %0, c9, c12, 1" : : "r"(cntens));
}

static uint64_t clock_gettime_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ULL + (uint64_t)ts.tv_nsec;
}

static void probe_pmu(void) {
    struct sigaction sa, old_sa;
    sa.sa_handler = pmu_sigill_handler;
    sigemptyset(&sa.sa_mask);
    sa.sa_flags = 0;
    sigaction(SIGILL, &sa, &old_sa);

    if (sigsetjmp(pmu_probe_env, 1) == 0) {
        enable_cycle_counter();
        (void)read_pmccntr();
        pmu_available = 1;
    } else {
        pmu_available = 0;
    }

    sigaction(SIGILL, &old_sa, NULL);
}

uint64_t qpp_timer_tick(void) {
    if (!pmu_checked) {
        probe_pmu();
        pmu_checked = 1;
    }

    if (pmu_available) {
        return (uint64_t)read_pmccntr();
    }
    return clock_gettime_ns();
}
