/* High-resolution timer backend for Windows (any CPU arch).
 *
 * QueryPerformanceCounter is backed by the OS's best available clock
 * source (usually the invariant TSC under the hood on modern hardware)
 * and is guaranteed monotonic and consistent across cores by the OS
 * itself, unlike a raw RDTSC read. It needs no runtime fallback here.
 */

#include <stdint.h>
#include <windows.h>

uint64_t qpp_timer_tick(void) {
    LARGE_INTEGER counter;
    QueryPerformanceCounter(&counter);
    return (uint64_t)counter.QuadPart;
}
