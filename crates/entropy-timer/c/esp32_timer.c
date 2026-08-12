/* High-resolution timer backend for ESP32 (Xtensa LX6, dual-core Wi-Fi
 * part).
 *
 * Reads the CPU's CCOUNT special register directly via `rsr` instead of
 * going through esp_timer_get_time(). CCOUNT increments once per CPU
 * clock cycle, is free-running from reset, is per-core, and needs no
 * ESP-IDF runtime/driver initialization -- making it both the cheapest
 * and the highest-resolution tick source available on this target.
 *
 * It is a 32-bit counter (wraps roughly every ~27-53s depending on CPU
 * frequency), so callers must diff two ticks with wrapping arithmetic
 * rather than treating the return value as a calibrated timestamp.
 */

#include <stdint.h>

uint64_t qpp_timer_tick(void) {
    uint32_t ccount;
    __asm__ __volatile__("rsr %0, ccount" : "=r"(ccount));
    return (uint64_t)ccount;
}
