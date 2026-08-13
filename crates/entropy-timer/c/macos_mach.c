/* High-resolution timer backend for macOS on Apple silicon (aarch64).
 *
 * mach_absolute_time() reads the CPU's own timebase directly (on Apple
 * silicon cores this is a 1ns-per-tick counter), avoiding the extra
 * libc/VDSO indirection that clock_gettime() carries.
 */

#include <stdint.h>
#include <mach/mach_time.h>

uint64_t qpp_timer_tick(void) {
    return mach_absolute_time();
}
