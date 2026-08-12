/* High-resolution timer backend for 64-bit ARM (aarch64) Linux, e.g.
 * Raspberry Pi 4 running a 64-bit kernel.
 *
 * CNTVCT_EL0 (the ARM generic timer's virtual count register) is
 * readable directly from EL0/userspace: Linux always sets
 * CNTKCTL_EL1.EL0VCTEN at boot, so unlike the 32-bit ARM PMU path this
 * needs no probing or runtime fallback.
 */

#include <stdint.h>

uint64_t qpp_timer_tick(void) {
    uint64_t val;
    __asm__ __volatile__("mrs %0, cntvct_el0" : "=r"(val));
    return val;
}
