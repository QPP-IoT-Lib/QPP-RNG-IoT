/* High-resolution timer backend for the Arduino Uno and Arduino Nano
 * (both ATmega328P, AVR).
 *
 * The Arduino core's micros() is *not* high resolution: it runs off
 * Timer0 with a /64 prescaler, so it only advances every 4us at 16MHz.
 * Instead we drive Timer1 (16-bit) with no prescaling at all, so it
 * ticks once per CPU clock cycle -- the finest-grained timer this MCU
 * has. Timer1 overflows every 65536 cycles (~4.1ms at 16MHz), so a
 * software counter incremented in the overflow ISR extends it to 32
 * significant bits, which is ample for jitter measurement (only short
 * deltas between consecutive ticks are ever used).
 *
 * qpp_timer_init() must be called once, before the first qpp_timer_tick(),
 * to start Timer1 and enable its overflow interrupt.
 */

#include <stdint.h>
#include <avr/interrupt.h>
#include <avr/io.h>

static volatile uint16_t timer1_overflow_count = 0;

ISR(TIMER1_OVF_vect) {
    timer1_overflow_count++;
}

void qpp_timer_init(void) {
    uint8_t sreg = SREG;
    cli();

    TCCR1A = 0;                 /* normal mode, free-running */
    TCCR1B = (1 << CS10);       /* no prescaling: tick at F_CPU */
    TCNT1 = 0;
    timer1_overflow_count = 0;
    TIFR1 |= (1 << TOV1);       /* clear any stale overflow flag */
    TIMSK1 |= (1 << TOIE1);     /* enable overflow interrupt */

    SREG = sreg;
    if (sreg & (1 << SREG_I)) {
        sei();
    }
}

uint64_t qpp_timer_tick(void) {
    uint8_t sreg = SREG;
    cli();

    uint16_t low = TCNT1;
    uint16_t high = timer1_overflow_count;

    /* If an overflow happened right at/after the TCNT1 read but before
     * we could observe it above, account for it so `high`/`low` stay
     * consistent instead of momentarily jumping backwards. */
    if ((TIFR1 & (1 << TOV1)) && low < 0x8000) {
        high++;
    }

    SREG = sreg;

    return ((uint64_t)high << 16) | (uint64_t)low;
}
