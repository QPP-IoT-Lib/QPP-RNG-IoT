/* High-resolution timer backend for x86/x86_64 Linux and macOS.
 *
 * RDTSC is the cheapest possible read available on this architecture --
 * no syscall, no VDSO trampoline -- but it is only safe to compare
 * across cores/threads when the CPU advertises an *invariant* TSC
 * (constant rate, synchronized across cores, unaffected by P-state/C-
 * state transitions). That is checked once via CPUID leaf 0x80000007
 * and cached; when it's unavailable (older or some virtualized CPUs)
 * every call instead falls back to clock_gettime(CLOCK_MONOTONIC),
 * which both Linux and macOS provide. This one file therefore covers
 * both the "RDTSC" and the "Unix-like fallback" rows for this arch.
 */

#include <stdint.h>
#include <time.h>

#if defined(__x86_64__) || defined(__i386__)
#include <cpuid.h>
#endif

static int tsc_checked = 0;
static int tsc_invariant = 0;

static int has_invariant_tsc(void) {
#if defined(__x86_64__) || defined(__i386__)
    unsigned int eax, ebx, ecx, edx;

    if (__get_cpuid_max(0x80000000, NULL) < 0x80000007) {
        return 0;
    }
    __cpuid(0x80000007, eax, ebx, ecx, edx);
    return (edx & (1u << 8)) != 0; /* InvariantTSC (CPUID.80000007H:EDX[8]) */
#else
    return 0;
#endif
}

static uint64_t clock_gettime_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ULL + (uint64_t)ts.tv_nsec;
}

uint64_t qpp_timer_tick(void) {
    if (!tsc_checked) {
        tsc_invariant = has_invariant_tsc();
        tsc_checked = 1;
    }

#if defined(__x86_64__) || defined(__i386__)
    if (tsc_invariant) {
        unsigned int lo, hi;
        __asm__ __volatile__("rdtsc" : "=a"(lo), "=d"(hi));
        return ((uint64_t)hi << 32) | (uint64_t)lo;
    }
#endif

    return clock_gettime_ns();
}
