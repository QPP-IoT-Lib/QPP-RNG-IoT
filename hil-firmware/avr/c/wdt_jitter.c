/* Cross-clock jitter source for AVR: the watchdog timer runs off its own
 * ~128 kHz RC oscillator, independent of the 16 MHz crystal/resonator
 * that clocks the CPU and Timer1. Timestamping each watchdog interrupt
 * with Timer1 (via entropy-timer's qpp_timer_tick) therefore measures
 * the RC oscillator's period against the crystal, and the RC
 * oscillator's thermal phase noise accumulates over the ~16 ms period
 * into the low bits of that measurement -- unlike anything timed purely
 * in CPU cycles, which on this core is fully deterministic.
 *
 * Captured timestamps go into a small single-producer/single-consumer
 * ring that qpp_wdt_take() drains from the main loop.
 */

#include <stdint.h>
#include <avr/interrupt.h>
#include <avr/io.h>

extern uint64_t qpp_timer_tick(void);

#define RING 16u

static volatile uint32_t ring[RING];
static volatile uint8_t head = 0;
static volatile uint8_t tail = 0;

ISR(WDT_vect) {
    uint32_t t = (uint32_t)qpp_timer_tick();
    uint8_t next = (uint8_t)((head + 1u) % RING);
    if (next != tail) {
        ring[head] = t;
        head = next;
    }
}

/* Watchdog in interrupt-only mode (no reset), shortest period (~16 ms). */
void qpp_wdt_init(void) {
    uint8_t sreg = SREG;
    cli();
    MCUSR &= (uint8_t)~(1 << WDRF);
    /* Timed sequence: WDCE|WDE, then the new value within 4 cycles. */
    WDTCSR = (1 << WDCE) | (1 << WDE);
    WDTCSR = (1 << WDIE);
    SREG = sreg;
}

/* Returns 1 and writes the next captured Timer1 timestamp to *out, or
 * returns 0 if no new watchdog interrupt has fired since the last call. */
uint8_t qpp_wdt_take(uint32_t *out) {
    uint8_t ok = 0;
    uint8_t sreg = SREG;
    cli();
    if (tail != head) {
        *out = ring[tail];
        tail = (uint8_t)((tail + 1u) % RING);
        ok = 1;
    }
    SREG = sreg;
    return ok;
}
