// nixxa: SSD1306 over I2C and framebuffer drawing. See nx_display.h.
#include "nx_display.h"

#include <Arduino.h>
#include <Wire.h>
#include <string.h>

#include "nx_font5x7.h"
#include "qrcodegen.h"

static uint8_t addr;
static uint32_t clockHz;
static size_t wireChunk = 32; // payload bytes per transfer after the control byte

// ---------------------------------------------------------------------------
// Panel
// ---------------------------------------------------------------------------
static bool probe(uint8_t a)
{
  Wire.beginTransmission(a);
  return Wire.endTransmission() == 0;
}

static void command(uint8_t c)
{
  Wire.beginTransmission(addr);
  Wire.write((uint8_t)0x00); // control byte: commands follow
  Wire.write(c);
  Wire.endTransmission();
}

uint8_t nxDisplayBegin(int sda, int scl)
{
  // A whole page (control byte + 128 data bytes) in one transfer when the
  // driver accepts a larger buffer; otherwise split it.
  if (Wire.setBufferSize(256) >= 129) wireChunk = 128;
  if (!Wire.begin(sda, scl, 400000)) return 0;
  Wire.setTimeOut(20); // ms: a missing or stuck panel can't hang the loop
  static const uint8_t addrs[2] = { 0x3C, 0x3D };
  addr = 0;
  for (uint8_t i = 0; i < 2 && !addr; i++)
    if (probe(addrs[i])) addr = addrs[i];
  if (!addr) {
    Wire.end();
    return 0;
  }
  clockHz = 400000;
  // Many modules run fine at 1 MHz; keep it only if the panel still ACKs.
  if (Wire.setClock(1000000) && probe(addr)) clockHz = 1000000;
  else Wire.setClock(400000);

  // Init sequence from the original working sketch (same as the AVR firmware).
  static const uint8_t init[] = { 0xAE, 0xD5, 0x80, 0xA8, 0x3F, 0xD3, 0x00, 0x40, 0x8D, 0x14, 0x20, 0x02,
                                  0xA1, 0xC8, 0xDA, 0x12, 0x81, 0xCF, 0xD9, 0xF1, 0xDB, 0x40, 0xA4, 0xA6, 0xAF };
  for (size_t i = 0; i < sizeof(init); i++) command(init[i]);
  return addr;
}

uint8_t nxDisplayAddr() { return addr; }
uint32_t nxDisplayClock() { return clockHz; }

void nxDisplayConfig(uint8_t contrast, bool invert, bool rotate180)
{
  if (!addr) return;
  command(0x81);
  command(contrast);
  command(rotate180 ? 0xA0 : 0xA1);
  command(rotate180 ? 0xC0 : 0xC8);
  command(invert ? 0xA7 : 0xA6);
}

void nxDisplayPush(const uint8_t *fb)
{
  if (!addr) return;
  for (uint8_t page = 0; page < 8; page++) {
    Wire.beginTransmission(addr);
    Wire.write((uint8_t)0x00);
    Wire.write((uint8_t)(0xB0 + page));
    Wire.write((uint8_t)0x00); // column 0, low nibble
    Wire.write((uint8_t)0x10); // column 0, high nibble
    Wire.endTransmission();
    const uint8_t *p = fb + page * 128;
    for (size_t off = 0; off < 128; off += wireChunk) {
      size_t n = 128 - off < wireChunk ? 128 - off : wireChunk;
      Wire.beginTransmission(addr);
      Wire.write((uint8_t)0x40); // control byte: data follows
      Wire.write(p + off, n);
      Wire.endTransmission();
    }
  }
}

// ---------------------------------------------------------------------------
// Framebuffer
// ---------------------------------------------------------------------------
void nxFbClear(uint8_t *fb) { memset(fb, 0, 1024); }

void nxFbSet(uint8_t *fb, int x, int y, bool on)
{
  if (x < 0 || x >= 128 || y < 0 || y >= 64) return;
  uint8_t m = (uint8_t)(1u << (y & 7));
  if (on) fb[(y >> 3) * 128 + x] |= m;
  else fb[(y >> 3) * 128 + x] &= (uint8_t)~m;
}

void nxFbFill(uint8_t *fb, int x, int y, int w, int h, bool on)
{
  for (int j = y; j < y + h; j++)
    for (int i = x; i < x + w; i++) nxFbSet(fb, i, j, on);
}

void nxFbHLine(uint8_t *fb, int x0, int x1, int y)
{
  for (int x = x0; x <= x1; x++) nxFbSet(fb, x, y, true);
}

int nxTextWidth(size_t n) { return n ? (int)n * 6 - 1 : 0; }

static void glyph(uint8_t *fb, int x, int y, char c, int sy, int clipTop, int clipBot, bool on, bool big)
{
  static const uint8_t kBigW[5] = { 2, 1, 2, 1, 2 };
  if ((uint8_t)c < NX_FONT_FIRST || (uint8_t)c > NX_FONT_LAST) c = '?';
  const uint8_t *g = NX_FONT5X7 + ((uint8_t)c - NX_FONT_FIRST) * 5;
  for (int col = 0; col < 5; col++) {
    uint8_t bits = g[col];
    int w = big ? kBigW[col] : 1;
    for (int row = 0; row < 7; row++) {
      if (!(bits & (1u << row))) continue;
      for (int dy = 0; dy < sy; dy++) {
        int py = y + row * sy + dy;
        if (py < clipTop || py >= clipBot) continue;
        for (int dx = 0; dx < w; dx++) nxFbSet(fb, x + dx, py, on);
      }
    }
    x += w;
  }
}

int nxText(uint8_t *fb, int x, int y, const char *s, size_t n, int clipTop, int clipBot, bool on)
{
  if (y + 7 <= clipTop || y >= clipBot) return x + (int)n * 6;
  for (size_t i = 0; i < n && s[i]; i++, x += 6) glyph(fb, x, y, s[i], 1, clipTop, clipBot, on, false);
  return x;
}

// big: double height, columns 2,1,2,1,2 px wide (9 px per glyph), as on AVR.
void nxTextCentered(uint8_t *fb, int y, const char *s, bool big)
{
  int adv = big ? 9 : 6;
  int len = (int)strlen(s);
  int x = (128 - (len * adv - 1)) / 2;
  for (; *s; s++, x += adv) glyph(fb, x, y, *s, big ? 2 : 1, 0, 64, true, big);
}

void nxIcon16(uint8_t *fb, int x, int y, const uint8_t icon[32])
{
  for (int r = 0; r < 16; r++)
    for (int c = 0; c < 16; c++)
      if (icon[r * 2 + (c >> 3)] & (0x80u >> (c & 7))) nxFbSet(fb, x + c, y + r, true);
}

void nxIcon32(uint8_t *fb, int x, int y, const uint8_t icon[128])
{
  for (int r = 0; r < 32; r++)
    for (int c = 0; c < 32; c++)
      if (icon[r * 4 + (c >> 3)] & (0x80u >> (c & 7))) nxFbSet(fb, x + c, y + r, true);
}

const uint8_t NX_ICON_BELL[32] = {
  0x01, 0x80, 0x03, 0xC0, 0x07, 0xE0, 0x0F, 0xF0, 0x0F, 0xF0, 0x1F, 0xF8, 0x1F, 0xF8, 0x1F, 0xF8,
  0x1F, 0xF8, 0x3F, 0xFC, 0x7F, 0xFE, 0xFF, 0xFF, 0x00, 0x00, 0x03, 0xC0, 0x01, 0x80, 0x00, 0x00
};

const uint8_t NX_ICON_PHONE[32] = {
  0x60, 0x00, 0xF0, 0x00, 0xF0, 0x00, 0xF0, 0x00, 0x70, 0x00, 0x38, 0x00, 0x1C, 0x00, 0x0E, 0x00,
  0x07, 0x00, 0x03, 0x80, 0x01, 0xC6, 0x00, 0xEF, 0x00, 0x7F, 0x00, 0x3F, 0x00, 0x1E, 0x00, 0x00
};

// ---------------------------------------------------------------------------
// QR
// ---------------------------------------------------------------------------
static uint8_t qr[qrcodegen_BUFFER_LEN_FOR_VERSION(3)];
static bool qrOk;

bool nxQrSet(const char *text)
{
  uint8_t tmp[qrcodegen_BUFFER_LEN_FOR_VERSION(3)];
  qrOk = false;
  if (!qrcodegen_isAlphanumeric(text) ||
      qrcodegen_calcSegmentBufferSize(qrcodegen_Mode_ALPHANUMERIC, strlen(text)) > sizeof(tmp))
    return false;
  struct qrcodegen_Segment seg = qrcodegen_makeAlphanumeric(text, tmp);
  // boostEcl off: the contract fixes ECC M (boosting would pick Q at version 3).
  qrOk = qrcodegen_encodeSegmentsAdvanced(&seg, 1, qrcodegen_Ecc_MEDIUM, 1, 3, qrcodegen_Mask_AUTO, false, tmp, qr);
  return qrOk;
}

int nxQrSize() { return qrOk ? qrcodegen_getSize(qr) : 0; }

void nxQrDraw(uint8_t *fb)
{
  if (!qrOk) return;
  int n = qrcodegen_getSize(qr);
  int px = n * 2;
  int x0 = (64 - px) / 2, y0 = (64 - px) / 2; // 29 modules: a 3 px quiet zone
  nxFbFill(fb, 0, 0, 64, 64, true);           // light background
  for (int y = 0; y < n; y++)
    for (int x = 0; x < n; x++)
      if (qrcodegen_getModule(qr, x, y)) nxFbFill(fb, x0 + 2 * x, y0 + 2 * y, 2, 2, false);
}
