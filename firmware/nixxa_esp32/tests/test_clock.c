/* Host tests for the clock faces (nx_clock.c).
 *   make -C firmware/nixxa_esp32/tests
 */
#include <stdio.h>
#include <string.h>
#include <stdint.h>

#include "../nx_clock.h"

static int fails;
#define CHECK(c)                                                              \
  do {                                                                        \
    if (!(c)) {                                                               \
      fprintf(stderr, "%s:%d: CHECK failed: %s\n", __FILE__, __LINE__, #c);   \
      fails++;                                                                \
    }                                                                         \
  } while (0)

typedef struct {
  int h, m, s;
} tm_case_t;

static const tm_case_t TIMES[] = {
  {0, 0, 0}, {9, 5, 7}, {12, 30, 0}, {23, 59, 59}, {10, 8, 30}, {17, 45, 12},
};
#define NTIMES ((int)(sizeof(TIMES) / sizeof(TIMES[0])))

static void draw(uint8_t *fb, uint8_t style, int h, int m, int s, int have_time)
{
  nx_clock_time_t t;
  t.hour = (uint8_t)h;
  t.minute = (uint8_t)m;
  t.second = (uint8_t)s;
  nx_clock_draw(fb, style, &t, have_time);
}

static int lit_count(const uint8_t *fb)
{
  int i, n = 0;
  for (i = 0; i < 1024; i++) {
    uint8_t b = fb[i];
    while (b) {
      n += b & 1;
      b >>= 1;
    }
  }
  return n;
}

/* Every style, several times: the buffer is cleared before drawing (stale
 * garbage does not leak through), and the result is neither blank nor full. */
static void test_draw_clears_and_draws(void)
{
  uint8_t a[1024], b[1024];
  int style, k;
  for (style = 1; style <= NX_CLOCK_STYLES; style++) {
    for (k = 0; k < NTIMES; k++) {
      int n;
      memset(a, 0x00, sizeof(a));
      memset(b, 0xFF, sizeof(b));
      draw(a, (uint8_t)style, TIMES[k].h, TIMES[k].m, TIMES[k].s, 1);
      draw(b, (uint8_t)style, TIMES[k].h, TIMES[k].m, TIMES[k].s, 1);
      CHECK(memcmp(a, b, sizeof(a)) == 0);
      n = lit_count(a);
      CHECK(n > 0);
      CHECK(n < 1024 * 8);
    }
    {
      int n;
      memset(a, 0xFF, sizeof(a));
      draw(a, (uint8_t)style, 0, 0, 0, 0);
      n = lit_count(a);
      CHECK(n > 0);
      CHECK(n < 1024 * 8);
    }
  }
}

/* The unset state (have_time == 0) must look different from a set clock. */
static void test_unset_differs(void)
{
  uint8_t set[1024], unset[1024];
  int style, k;
  for (style = 1; style <= NX_CLOCK_STYLES; style++) {
    for (k = 0; k < NTIMES; k++) {
      draw(set, (uint8_t)style, TIMES[k].h, TIMES[k].m, TIMES[k].s, 1);
      draw(unset, (uint8_t)style, TIMES[k].h, TIMES[k].m, TIMES[k].s, 0);
      CHECK(memcmp(set, unset, sizeof(set)) != 0);
    }
  }
}

/* Unset output does not depend on the time passed in, and NULL is allowed. */
static void test_unset_ignores_time(void)
{
  uint8_t a[1024], b[1024];
  int style;
  for (style = 1; style <= NX_CLOCK_STYLES; style++) {
    nx_clock_draw(a, (uint8_t)style, NULL, 0);
    draw(b, (uint8_t)style, 14, 37, 9, 0);
    CHECK(memcmp(a, b, sizeof(a)) == 0);
  }
}

/* Styles outside 1..NX_CLOCK_STYLES draw style 1. */
static void test_out_of_range_is_style_one(void)
{
  static const uint8_t bad[] = {0, 21, 255};
  uint8_t ref[1024], got[1024];
  size_t i;
  draw(ref, 1, 10, 8, 0, 1);
  for (i = 0; i < sizeof(bad); i++) {
    draw(got, bad[i], 10, 8, 0, 1);
    CHECK(memcmp(ref, got, sizeof(ref)) == 0);
  }
  nx_clock_draw(got, 0, NULL, 0);
  nx_clock_draw(ref, 1, NULL, 0);
  CHECK(memcmp(ref, got, sizeof(ref)) == 0);
}

static void test_names(void)
{
  int style;
  for (style = 1; style <= NX_CLOCK_STYLES; style++) {
    const char *name = nx_clock_name((uint8_t)style);
    CHECK(name != NULL);
    CHECK(name && name[0] != '\0');
  }
  CHECK(nx_clock_name(0) != NULL);
  CHECK(nx_clock_name(255) != NULL);
  CHECK(strcmp(nx_clock_name(0), nx_clock_name(1)) == 0);
}

/* At one time every style must render differently from every other style. */
static void test_styles_distinct(void)
{
  static uint8_t frames[NX_CLOCK_STYLES][1024];
  int i, j;
  for (i = 0; i < NX_CLOCK_STYLES; i++) draw(frames[i], (uint8_t)(i + 1), 10, 8, 30, 1);
  for (i = 0; i < NX_CLOCK_STYLES; i++)
    for (j = i + 1; j < NX_CLOCK_STYLES; j++)
      CHECK(memcmp(frames[i], frames[j], 1024) != 0);
}

/* Sweep many times under the sanitizers: edge hours, minutes and seconds. */
static void test_time_sweep(void)
{
  static uint8_t fb[1024];
  int style, h, m, s;
  for (style = 1; style <= NX_CLOCK_STYLES; style++)
    for (h = 0; h < 24; h++)
      for (m = 0; m < 60; m += 7)
        for (s = 0; s < 60; s += 29) draw(fb, (uint8_t)style, h, m, s, 1);
  for (style = 1; style <= NX_CLOCK_STYLES; style++) {
    draw(fb, (uint8_t)style, 23, 59, 59, 1);
    draw(fb, (uint8_t)style, 0, 0, 0, 1);
    draw(fb, (uint8_t)style, 12, 0, 0, 1);
  }
}

int main(void)
{
  test_draw_clears_and_draws();
  test_unset_differs();
  test_unset_ignores_time();
  test_out_of_range_is_style_one();
  test_names();
  test_styles_distinct();
  test_time_sweep();
  if (fails) {
    fprintf(stderr, "test_clock: %d check(s) failed\n", fails);
    return 1;
  }
  printf("test_clock: all checks passed\n");
  return 0;
}
