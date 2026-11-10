// nixxa: SSD1306 128x64 over I2C (Wire) plus the framebuffer drawing used for
// the QR screen, notification cards and the call screen. The face itself is
// drawn by the DeskBunny engine. Framebuffer: SSD1306 page layout, 1 = lit.
#pragma once

#include <stddef.h>
#include <stdint.h>

// Probe 0x3C then 0x3D at 400 kHz, try 1 MHz, send the AVR init sequence.
// Returns the address found (0: no panel; drawing calls are then no-ops).
uint8_t nxDisplayBegin(int sda, int scl);
uint8_t nxDisplayAddr();
uint32_t nxDisplayClock();
void nxDisplayPush(const uint8_t *fb);
// Contrast, invert (0xA7) and rotate180 (0xA0/0xC0) are done by the panel.
void nxDisplayConfig(uint8_t contrast, bool invert, bool rotate180);

void nxFbClear(uint8_t *fb);
void nxFbSet(uint8_t *fb, int x, int y, bool on);
void nxFbFill(uint8_t *fb, int x, int y, int w, int h, bool on);
void nxFbHLine(uint8_t *fb, int x0, int x1, int y);

// 5x7 text, 6 px per character. Pixels outside rows [clipTop, clipBot) are
// skipped. Characters outside 0x20..0x7E are drawn as '?'. Returns the end x.
int nxText(uint8_t *fb, int x, int y, const char *s, size_t n, int clipTop = 0, int clipBot = 64, bool on = true);
int nxTextWidth(size_t n);
void nxTextCentered(uint8_t *fb, int y, const char *s, bool big = false);
// 16x16, 1 bpp, row-major, MSB first, 1 = lit (the NOTIFY icon format).
void nxIcon16(uint8_t *fb, int x, int y, const uint8_t icon[32]);
// 32x32, 1 bpp, row-major, MSB first, 1 = lit (the app logo sent on NOTIFY).
void nxIcon32(uint8_t *fb, int x, int y, const uint8_t icon[128]);
extern const uint8_t NX_ICON_BELL[32];
extern const uint8_t NX_ICON_PHONE[32];

// QR for text (alphanumeric, ECC M, version <= 3). nxQrSet returns false if it
// doesn't fit; nxQrDraw draws it at 2 px per module, dark modules on a lit
// square at the left edge (x 0..63).
bool nxQrSet(const char *text);
void nxQrDraw(uint8_t *fb);
int nxQrSize();
