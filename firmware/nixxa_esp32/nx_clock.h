/* nixxa clock faces: 20 selectable styles drawn into the 128x64 SSD1306
 * page buffer. Pure C99, no Arduino or ESP-IDF includes, so the same code
 * builds on the host for the tests.
 *
 * Framebuffer: 1024 bytes, 128 columns x 8 pages, byte index = page*128 + x,
 * bit (y & 7) = row y within that page, bit set = pixel on.
 */
#ifndef NX_CLOCK_H
#define NX_CLOCK_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define NX_CLOCK_STYLES 20 /* styles are numbered 1..20 on the wire */

typedef struct {
  uint8_t hour, minute, second; /* hour 0..23 */
} nx_clock_time_t;

/* Clears fb, then draws style (1..NX_CLOCK_STYLES; anything else draws style 1)
 * for time t. have_time == 0 means the clock is not set yet: the style is drawn
 * with "--:--" in place of the time (t may then be NULL). */
void nx_clock_draw(uint8_t fb[1024], uint8_t style, const nx_clock_time_t *t, int have_time);

/* Short English name of a style, e.g. "Big digits". Never NULL; styles outside
 * 1..NX_CLOCK_STYLES return the name of style 1. */
const char *nx_clock_name(uint8_t style);

#ifdef __cplusplus
}
#endif

#endif
