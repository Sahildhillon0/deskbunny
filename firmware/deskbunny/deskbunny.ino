// DeskBunny firmware: animated face on a 128x64 SSD1306 over bit-banged SPI.
// Contract: docs/ARCHITECTURE.md. No libraries beyond the Arduino core.
//
// The face engine (deskbunny_engine.c/.h) lives in engine/; firmware/build.sh
// copies it next to this sketch before compiling. To build by hand, copy
// engine/deskbunny_engine.c and engine/deskbunny_engine.h into this folder.
//
// Pins (every board): CLK 13, MOSI 11, CS 10, DC 9, RES 8.

#include <Arduino.h>
#include "deskbunny_engine.h"

#define DB_FW_VERSION "0.1.2"

#if defined(__AVR__)
#include <avr/pgmspace.h>
#define DB_ROM PROGMEM
#define DB_ROM_BYTE(p) pgm_read_byte(p)
#else
#define DB_ROM
#define DB_ROM_BYTE(p) (*(const volatile uint8_t *)(p))
#endif

#include "db_default_blob.h"
#include "db_font5x7.h"

// Image id reported on the serial banner. build.sh passes -DDB_BOARD_ID=<image
// stem> (atmega328p, atmega2560, atmega32u4, uno_r4_minima, uno_r4_wifi).
#define DB_STR2(x) #x
#define DB_STR(x) DB_STR2(x)
#if defined(DB_BOARD_ID)
#define DB_BOARD_ID_STR DB_STR(DB_BOARD_ID)
#elif defined(ARDUINO_UNOR4_WIFI)
#define DB_BOARD_ID_STR "uno_r4_wifi"
#elif defined(ARDUINO_UNOR4_MINIMA)
#define DB_BOARD_ID_STR "uno_r4_minima"
#elif defined(__AVR_ATmega2560__)
#define DB_BOARD_ID_STR "atmega2560"
#elif defined(__AVR_ATmega32U4__)
#define DB_BOARD_ID_STR "atmega32u4"
#elif defined(__AVR_ATmega328P__)
#define DB_BOARD_ID_STR "atmega328p"
#else
#define DB_BOARD_ID_STR "unknown"
#endif

#if defined(USBCON) || defined(ARDUINO_UNOR4_MINIMA)
#define DB_NATIVE_USB 1 // Serial is USB CDC: never wait for the host
#else
#define DB_NATIVE_USB 0
#endif

// ---------------------------------------------------------------------------
// The face blob. The desktop app finds the "DBFACE01" magic in the flashed
// image and overwrites these 64 bytes, so it must exist exactly once, as real
// bytes in flash, and be read at runtime (never constant-folded).
// AVR: PROGMEM + pgm_read_byte (an lpm the compiler cannot see through).
// ARM: volatile const, read through a volatile pointer.
// ---------------------------------------------------------------------------
#if defined(__AVR__)
extern const uint8_t DB_FACE_BLOB[DB_BLOB_SIZE] PROGMEM;
__attribute__((used)) const uint8_t DB_FACE_BLOB[DB_BLOB_SIZE] PROGMEM = DB_DEFAULT_BLOB_INIT;
#else
extern volatile const uint8_t DB_FACE_BLOB[DB_BLOB_SIZE];
__attribute__((used)) volatile const uint8_t DB_FACE_BLOB[DB_BLOB_SIZE] = DB_DEFAULT_BLOB_INIT;
#endif

static const uint8_t PIN_OLED_CLK = 13;
static const uint8_t PIN_OLED_MOSI = 11;
static const uint8_t PIN_OLED_CS = 10;
static const uint8_t PIN_OLED_DC = 9;
static const uint8_t PIN_OLED_RES = 8;

static const uint32_t FRAME_MS = 33; // ~30 fps

static uint8_t fb[DB_FB_SIZE];
static db_engine_t engine;
static db_face_t face; // as decoded, including the invert/rotate180 flags
static uint32_t lastFrame;

// ---------------------------------------------------------------------------
// Bit-banged SPI. On AVR the pins are driven through their PORT registers
// (resolved from the core's pin tables, so any AVR board maps correctly);
// other cores use digitalWrite. SSD1306 samples MOSI on the rising CLK edge,
// CLK idles high, as in the original sketch.
// ---------------------------------------------------------------------------
#if defined(__AVR__)
static volatile uint8_t *clkReg, *mosiReg, *csReg, *dcReg;
static uint8_t clkBit, mosiBit, csBit, dcBit;

static void pinSetup(uint8_t pin, volatile uint8_t **reg, uint8_t *bit)
{
  *reg = portOutputRegister(digitalPinToPort(pin));
  *bit = digitalPinToBitMask(pin);
}

// Read-modify-write of a PORT register; interrupts are masked so an ISR that
// owns another pin of the same port (e.g. the 32u4 RX LED) can't be undone.
static inline void pinWrite(volatile uint8_t *reg, uint8_t bit, bool high)
{
  uint8_t sreg = SREG;
  cli();
  if (high) *reg |= bit;
  else *reg &= (uint8_t)~bit;
  SREG = sreg;
}

static void spiByte(uint8_t d)
{
  uint8_t sreg = SREG;
  cli();
  if (clkReg == mosiReg) {
    // Uno/Nano/Pro Mini (PB5/PB3) and Mega (PB7/PB5): one store per edge.
    uint8_t lo = (uint8_t)(*clkReg & (uint8_t)~(clkBit | mosiBit));
    uint8_t hi = (uint8_t)(lo | mosiBit);
    for (uint8_t m = 0x80; m; m >>= 1) {
      uint8_t v = (d & m) ? hi : lo;
      *clkReg = v;                    // CLK low, data out
      *clkReg = (uint8_t)(v | clkBit); // rising edge latches the bit
    }
  } else {
    // Leonardo/Micro: CLK (PC7) and MOSI (PB7) are on different ports.
    for (uint8_t m = 0x80; m; m >>= 1) {
      *clkReg &= (uint8_t)~clkBit;
      if (d & m) *mosiReg |= mosiBit;
      else *mosiReg &= (uint8_t)~mosiBit;
      *clkReg |= clkBit;
    }
  }
  SREG = sreg;
}

#define CS_LOW() pinWrite(csReg, csBit, false)
#define CS_HIGH() pinWrite(csReg, csBit, true)
#define DC_CMD() pinWrite(dcReg, dcBit, false)
#define DC_DATA() pinWrite(dcReg, dcBit, true)
#else
static void spiByte(uint8_t d)
{
  for (int8_t i = 7; i >= 0; i--) {
    digitalWrite(PIN_OLED_CLK, LOW);
    digitalWrite(PIN_OLED_MOSI, (d & (1u << i)) ? HIGH : LOW);
    digitalWrite(PIN_OLED_CLK, HIGH);
  }
}

#define CS_LOW() digitalWrite(PIN_OLED_CS, LOW)
#define CS_HIGH() digitalWrite(PIN_OLED_CS, HIGH)
#define DC_CMD() digitalWrite(PIN_OLED_DC, LOW)
#define DC_DATA() digitalWrite(PIN_OLED_DC, HIGH)
#endif

static void oledCommand(uint8_t cmd)
{
  CS_LOW();
  DC_CMD();
  spiByte(cmd);
  CS_HIGH();
}

// Init sequence from the original working sketch (firmware/reference/).
static void oledInit()
{
  pinMode(PIN_OLED_CLK, OUTPUT);
  pinMode(PIN_OLED_MOSI, OUTPUT);
  pinMode(PIN_OLED_CS, OUTPUT);
  pinMode(PIN_OLED_DC, OUTPUT);
  pinMode(PIN_OLED_RES, OUTPUT);
#if defined(__AVR__)
  pinSetup(PIN_OLED_CLK, &clkReg, &clkBit);
  pinSetup(PIN_OLED_MOSI, &mosiReg, &mosiBit);
  pinSetup(PIN_OLED_CS, &csReg, &csBit);
  pinSetup(PIN_OLED_DC, &dcReg, &dcBit);
#endif
  digitalWrite(PIN_OLED_CS, HIGH);
  digitalWrite(PIN_OLED_CLK, HIGH);
  digitalWrite(PIN_OLED_RES, HIGH); delay(10);
  digitalWrite(PIN_OLED_RES, LOW);  delay(50);
  digitalWrite(PIN_OLED_RES, HIGH); delay(50);
  oledCommand(0xAE);
  oledCommand(0xD5); oledCommand(0x80);
  oledCommand(0xA8); oledCommand(0x3F);
  oledCommand(0xD3); oledCommand(0x00);
  oledCommand(0x40);
  oledCommand(0x8D); oledCommand(0x14);   // charge pump on
  oledCommand(0x20); oledCommand(0x02);   // page addressing
  oledCommand(0xA1);
  oledCommand(0xC8);
  oledCommand(0xDA); oledCommand(0x12);
  oledCommand(0x81); oledCommand(0xCF);
  oledCommand(0xD9); oledCommand(0xF1);
  oledCommand(0xDB); oledCommand(0x40);
  oledCommand(0xA4);
  oledCommand(0xA6);
  oledCommand(0xAF);
}

// Contrast, invert and rotate180 are done by the panel, not by the engine.
static void oledApplyFace(const db_face_t *f)
{
  oledCommand(0x81);
  oledCommand(f->contrast);
  if (f->flags & DB_FLAG_ROTATE180) {
    oledCommand(0xA0); // column 0 -> SEG0
    oledCommand(0xC0); // COM0 -> COM63
  } else {
    oledCommand(0xA1);
    oledCommand(0xC8);
  }
  oledCommand((f->flags & DB_FLAG_INVERT) ? 0xA7 : 0xA6);
}

static void oledPush(const uint8_t *buf)
{
  for (uint8_t page = 0; page < 8; page++) {
    oledCommand((uint8_t)(0xB0 + page));
    oledCommand(0x00); // column 0, low nibble
    oledCommand(0x10); // column 0, high nibble
    CS_LOW();
    DC_DATA();
    const uint8_t *p = buf + (uint16_t)page * DB_W;
    for (uint8_t x = 0; x < DB_W; x++) spiByte(p[x]);
    CS_HIGH();
  }
}

// ---------------------------------------------------------------------------
// Boot splash
// ---------------------------------------------------------------------------
static void fbSet(int16_t x, int16_t y)
{
  if (x < 0 || x >= DB_W || y < 0 || y >= DB_H) return;
  fb[(uint16_t)(y >> 3) * DB_W + x] |= (uint8_t)(1u << (y & 7));
}

static void drawText(const char *s, int16_t y, uint8_t scale)
{
  int16_t len = (int16_t)strlen(s);
  int16_t x = (int16_t)((DB_W - (len * 6 - 1) * scale) / 2);
  for (; *s; s++, x = (int16_t)(x + 6 * scale)) {
    char c = *s;
    if (c >= 'a' && c <= 'z') c = (char)(c - 'a' + 'A');
    if (c < DB_FONT_FIRST || c > DB_FONT_LAST) c = ' ';
    const uint8_t *g = DB_FONT5X7 + (uint16_t)(c - DB_FONT_FIRST) * 5;
    for (uint8_t col = 0; col < 5; col++) {
      uint8_t bits = DB_ROM_BYTE(g + col);
      for (uint8_t row = 0; row < 7; row++) {
        if (!(bits & (1u << row))) continue;
        for (uint8_t dy = 0; dy < scale; dy++)
          for (uint8_t dx = 0; dx < scale; dx++)
            fbSet((int16_t)(x + col * scale + dx), (int16_t)(y + row * scale + dy));
      }
    }
  }
}

static void showSplash()
{
  memset(fb, 0, sizeof(fb));
  drawText("DESKBUNNY", 14, 2);
  // the face's name, or the firmware version when the name says nothing new
  if (face.name[0] && strcmp(face.name, "DESKBUNNY") != 0) drawText(face.name, 40, 1);
  else drawText("V" DB_FW_VERSION, 40, 1);
  oledPush(fb);
  delay(1200);
}

// ---------------------------------------------------------------------------
// Serial: banner at boot and in reply to "?\n"
// ---------------------------------------------------------------------------
static void printBanner()
{
#if DB_NATIVE_USB
  if (!Serial) return; // nobody listening: don't risk a blocking write
#endif
  Serial.print(F("DESKBUNNY " DB_FW_VERSION " " DB_BOARD_ID_STR " "));
  Serial.println(face.name);
}

static void pollSerial()
{
  static bool query = false;
  while (Serial.available() > 0) {
    int c = Serial.read();
    if (c == '?') {
      query = true;
    } else if (c == '\n' || c == '\r') {
      if (query) printBanner();
      query = false;
    } else {
      query = false;
    }
  }
}

#if DB_NATIVE_USB
// Native USB: the host usually opens the port after boot, so greet it once
// it shows up. Serial's bool operator costs ~10 ms on the 32u4: poll slowly.
static void announceWhenConnected(uint32_t now)
{
  static bool announced = false;
  static uint32_t lastCheck;
  if (announced || (uint32_t)(now - lastCheck) < 500) return;
  lastCheck = now;
  if (Serial) {
    announced = true;
    printBanner();
  }
}
#endif

// ---------------------------------------------------------------------------
void setup()
{
  uint8_t blob[DB_BLOB_SIZE];
  for (uint8_t i = 0; i < DB_BLOB_SIZE; i++) blob[i] = DB_ROM_BYTE(&DB_FACE_BLOB[i]);
  db_face_decode(blob, &face); // falls back to the default face on a bad blob

  Serial.begin(115200);
  printBanner();

  oledInit();
  oledApplyFace(&face);
  showSplash();

  // The panel does invert/rotate180 in hardware; the engine draws upright.
  db_face_t drawFace = face;
  drawFace.flags &= (uint8_t)~(DB_FLAG_INVERT | DB_FLAG_ROTATE180);
  uint32_t now = millis();
  db_engine_init(&engine, &drawFace, now ^ ((uint32_t)analogRead(A0) << 16));
  lastFrame = now - FRAME_MS;
}

void loop()
{
  pollSerial();
  uint32_t now = millis();
#if DB_NATIVE_USB
  announceWhenConnected(now);
#endif
  if ((uint32_t)(now - lastFrame) >= FRAME_MS) {
    lastFrame += FRAME_MS;
    if ((uint32_t)(now - lastFrame) >= FRAME_MS) lastFrame = now; // fell behind: resync
    db_engine_render(&engine, now, fb);
    oledPush(fb);
  }
}
