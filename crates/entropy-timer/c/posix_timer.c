/* Generic Unix-like fallback high-resolution timer backend.
 *
 * Used for any Unix-like target that doesn't have a more specific,
 * cheaper backend of its own (see build.rs for the full selection
 * order) -- e.g. Linux on riscv64/mips/powerpc, or a BSD. The more
 * specific x86/aarch64/32-bit-ARM backends also call back into this
 * same mechanism at runtime when their preferred fast path isn't
 * available on the running CPU.
 *
 * clock_gettime(CLOCK_MONOTONIC) needs no elevated privileges and is
 * nanosecond-resolution in practice (served from the VDSO on Linux).
 */

#include <stdint.h>
#include <time.h>

uint64_t qpp_timer_tick(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ULL + (uint64_t)ts.tv_nsec;
}
