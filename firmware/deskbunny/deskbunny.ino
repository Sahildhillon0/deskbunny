// DeskBunny firmware: animated face on a 128x64 SSD1306 over I2C or
// bit-banged SPI (detected at boot).
// Contract: docs/ARCHITECTURE.md. No libraries beyond the Arduino core.
//
// The face engine (deskbunny_engine.c/.h) lives in engine/; firmware/build.sh
// copies it next to this sketch before compiling. To build by hand, copy
// engine/deskbunny_engine.c and engine/deskbunny_engine.h into this folder.
//
// I2C: the board's SDA/SCL pins (Uno/Nano/Pro Mini/R4 A4/A5, Mega 20/21,
// Leonardo/Micro 2/3), address 0x3C or 0x3D, RES 8 if the module has one.
// SPI (every board): CLK 13, MOSI 11, CS 10, DC 9, RES 8.

#include <Arduino.h>
#if !defined(__AVR__)
#include <Wire.h>
#endif
#include "deskbunny_engine.h"

#define DB_FW_VERSION "0.1.9"

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

// The drawing blob (Drawing blob v1): up to 4 frames shown instead of or over
// the face. Same rules as the face blob; it ships "off" and the app patches it.
#define DB_ART_INIT { 'D', 'B', 'A', 'R', 'T', '0', '0', '1', DB_ART_VERSION }
#if defined(__AVR__)
extern const uint8_t DB_ART_BLOB[DB_ART_SIZE] PROGMEM;
__attribute__((used)) const uint8_t DB_ART_BLOB[DB_ART_SIZE] PROGMEM = DB_ART_INIT;
#else
extern volatile const uint8_t DB_ART_BLOB[DB_ART_SIZE];
__attribute__((used)) volatile const uint8_t DB_ART_BLOB[DB_ART_SIZE] = DB_ART_INIT;
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
static uint8_t artMode;   // DB_ART_OFF / REPLACE / OVER
static uint8_t artFrames; // 1..DB_ART_FRAMES when artMode != DB_ART_OFF
static uint16_t artMs;
static uint8_t i2cAddr; // 0: the panel is on SPI

// ---------------------------------------------------------------------------
// I2C. The panel's control byte says what follows: 0x00 commands, 0x40 data.
// AVR drives the TWI unit through its registers (no Wire: no 32-byte buffer,
// so a whole page goes out in one transfer); other cores use Wire. Every wait
// is bounded, so a missing or stuck panel can't hang the board.
// ---------------------------------------------------------------------------
static const uint8_t OLED_I2C_ADDRS[2] = { 0x3C, 0x3D };

#if defined(__AVR__)
static bool twiWait()
{
  uint16_t n = 0;
  while (!(TWCR & _BV(TWINT))) {
    if (++n == 0) return false;
  }
  return true;
}

static void twiStop()
{
  TWCR = _BV(TWINT) | _BV(TWSTO) | _BV(TWEN);
  uint16_t n = 0;
  while ((TWCR & _BV(TWSTO)) && ++n) {}
}

// START + address; true when the panel ACKs.
static bool twiStart(uint8_t addr)
{
  TWCR = _BV(TWINT) | _BV(TWSTA) | _BV(TWEN);
  if (!twiWait()) return false;
  uint8_t st = TWSR & 0xF8;
  if (st != 0x08 && st != 0x10) return false; // START / repeated START sent
  TWDR = (uint8_t)(addr << 1);
  TWCR = _BV(TWINT) | _BV(TWEN);
  if (!twiWait()) return false;
  return (TWSR & 0xF8) == 0x18; // SLA+W ACKed
}

static void twiByte(uint8_t d)
{
  TWDR = d;
  TWCR = _BV(TWINT) | _BV(TWEN);
  twiWait();
}

static void i2cBegin()
{
  pinMode(SDA, INPUT_PULLUP); // weak, but enough for a short run of wire
  pinMode(SCL, INPUT_PULLUP);
  TWSR = 0;                                     // prescaler 1
  TWBR = (uint8_t)((F_CPU / 400000UL - 16) / 2); // 400 kHz
  TWCR = _BV(TWEN);
}

static bool i2cProbe(uint8_t addr)
{
  bool ack = twiStart(addr);
  twiStop();
  return ack;
}

static void i2cEnd() { twiStop(); }
static void i2cRelease() { TWCR = 0; }

static void i2cOpen(uint8_t ctrl)
{
  twiStart(i2cAddr);
  twiByte(ctrl);
}

#define i2cByte(d) twiByte(d)
#else
#if defined(I2C_BUFFER_LENGTH) && I2C_BUFFER_LENGTH > 32
#define DB_WIRE_CHUNK (I2C_BUFFER_LENGTH > 255 ? 255 : I2C_BUFFER_LENGTH) // R4: 255
#else
#define DB_WIRE_CHUNK 32 // Wire's classic buffer
#endif
static uint8_t i2cCtrl, i2cCount;

static void i2cBegin()
{
  Wire.begin();
  Wire.setClock(400000);
}

static bool i2cProbe(uint8_t addr)
{
  Wire.beginTransmission(addr);
  return Wire.endTransmission() == 0;
}

static void i2cEnd() { Wire.endTransmission(); }
static void i2cRelease() { Wire.end(); }

static void i2cOpen(uint8_t ctrl)
{
  i2cCtrl = ctrl;
  Wire.beginTransmission(i2cAddr);
  Wire.write(ctrl);
  i2cCount = 1;
}

// Wire buffers each transfer; split long runs, repeating the control byte
// (the panel's column pointer carries on where it stopped).
static void i2cByte(uint8_t d)
{
  if (i2cCount == DB_WIRE_CHUNK) {
    Wire.endTransmission();
    i2cOpen(i2cCtrl);
  }
  Wire.write(d);
  i2cCount++;
}
#endif

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

// ---------------------------------------------------------------------------
// Panel transfers, whichever bus it is on.
// ---------------------------------------------------------------------------
static void oledOpen(bool data)
{
  if (i2cAddr) {
    i2cOpen(data ? 0x40 : 0x00);
  } else {
    CS_LOW();
    if (data) DC_DATA();
    else DC_CMD();
  }
}

static inline void oledByte(uint8_t d)
{
  if (i2cAddr) i2cByte(d);
  else spiByte(d);
}

static void oledClose()
{
  if (i2cAddr) i2cEnd();
  else CS_HIGH();
}

static void oledCommand(uint8_t cmd)
{
  oledOpen(false);
  oledByte(cmd);
  oledClose();
}

// Reset the panel, then look for it on I2C; nobody answering means SPI.
static void oledDetect()
{
  pinMode(PIN_OLED_RES, OUTPUT);
  digitalWrite(PIN_OLED_RES, HIGH); delay(10);
  digitalWrite(PIN_OLED_RES, LOW);  delay(50);
  digitalWrite(PIN_OLED_RES, HIGH); delay(50);

  i2cBegin();
  for (uint8_t i = 0; i < sizeof(OLED_I2C_ADDRS); i++) {
    if (i2cProbe(OLED_I2C_ADDRS[i])) {
      i2cAddr = OLED_I2C_ADDRS[i];
      return;
    }
  }
  i2cRelease();

  pinMode(PIN_OLED_CLK, OUTPUT);
  pinMode(PIN_OLED_MOSI, OUTPUT);
  pinMode(PIN_OLED_CS, OUTPUT);
  pinMode(PIN_OLED_DC, OUTPUT);
#if defined(__AVR__)
  pinSetup(PIN_OLED_CLK, &clkReg, &clkBit);
  pinSetup(PIN_OLED_MOSI, &mosiReg, &mosiBit);
  pinSetup(PIN_OLED_CS, &csReg, &csBit);
  pinSetup(PIN_OLED_DC, &dcReg, &dcBit);
#endif
  digitalWrite(PIN_OLED_CS, HIGH);
  digitalWrite(PIN_OLED_CLK, HIGH);
}

// Init sequence from the original working sketch (firmware/reference/).
static void oledInit()
{
  oledDetect();
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
    oledOpen(true);
    const uint8_t *p = buf + (uint16_t)page * DB_W;
    for (uint8_t x = 0; x < DB_W; x++) oledByte(p[x]);
    oledClose();
  }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------
static void artLoad()
{
  // XOR-ed so "DBART001" occurs in the image only once, in DB_ART_BLOB.
  static const uint8_t magicX[8] = { 'D' ^ 0x5A, 'B' ^ 0x5A, 'A' ^ 0x5A, 'R' ^ 0x5A,
                                     'T' ^ 0x5A, '0' ^ 0x5A, '0' ^ 0x5A, '1' ^ 0x5A };
  artMode = DB_ART_OFF;
  for (uint8_t i = 0; i < 8; i++)
    if ((uint8_t)(DB_ROM_BYTE(&DB_ART_BLOB[i]) ^ 0x5A) != magicX[i]) return;
  if (DB_ROM_BYTE(&DB_ART_BLOB[8]) != DB_ART_VERSION) return;
  uint8_t mode = DB_ROM_BYTE(&DB_ART_BLOB[9]);
  uint8_t frames = DB_ROM_BYTE(&DB_ART_BLOB[10]);
  if ((mode != DB_ART_REPLACE && mode != DB_ART_OVER) || frames < 1 || frames > DB_ART_FRAMES) return;
  uint16_t ms = (uint16_t)(DB_ROM_BYTE(&DB_ART_BLOB[12]) | (DB_ROM_BYTE(&DB_ART_BLOB[13]) << 8));
  if (ms == 0) ms = 500;
  artMs = ms < 50 ? 50 : (ms > 10000 ? 10000 : ms);
  artFrames = frames;
  artMode = mode;
}

// Replace or OR the current drawing frame into fb, reading it straight from flash.
static void artCompose(uint32_t now)
{
  uint8_t frame = (uint8_t)((now / artMs) % artFrames);
  uint16_t base = (uint16_t)(DB_ART_HEADER + (uint16_t)frame * DB_FB_SIZE);
  if (artMode == DB_ART_REPLACE) {
    for (uint16_t i = 0; i < DB_FB_SIZE; i++) fb[i] = DB_ROM_BYTE(&DB_ART_BLOB[base + i]);
  } else {
    for (uint16_t i = 0; i < DB_FB_SIZE; i++) fb[i] |= DB_ROM_BYTE(&DB_ART_BLOB[base + i]);
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

// Centred 5x7 text. big: double height, columns 2,1,2,1,2 px wide (8 px per
// glyph instead of 10), so twelve letters still fit across the 128 px panel.
static void drawText(const char *s, int16_t y, bool big)
{
  static const uint8_t kBigW[5] = { 2, 1, 2, 1, 2 };
  const uint8_t adv = big ? 9 : 6, sy = big ? 2 : 1;
  int16_t len = (int16_t)strlen(s);
  int16_t x = (int16_t)((DB_W - (len * adv - 1)) / 2);
  for (; *s; s++, x = (int16_t)(x + adv)) {
    char c = *s;
    if (c < DB_FONT_FIRST || c > DB_FONT_LAST) c = ' ';
    const uint8_t *g = DB_FONT5X7 + (uint16_t)(c - DB_FONT_FIRST) * 5;
    int16_t cx = x;
    for (uint8_t col = 0; col < 5; col++) {
      uint8_t bits = DB_ROM_BYTE(g + col), w = big ? kBigW[col] : 1;
      for (uint8_t row = 0; row < 7; row++) {
        if (!(bits & (1u << row))) continue;
        for (uint8_t dy = 0; dy < sy; dy++)
          for (uint8_t dx = 0; dx < w; dx++) fbSet((int16_t)(cx + dx), (int16_t)(y + row * sy + dy));
      }
      cx = (int16_t)(cx + w);
    }
  }
}

static void showSplash()
{
  memset(fb, 0, sizeof(fb));
  drawText("Best Regards", 17, false);
  drawText("Zenithratech", 31, true);
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
  artLoad();

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
    if (artMode != DB_ART_REPLACE) db_engine_render(&engine, now, fb);
    if (artMode != DB_ART_OFF) artCompose(now);
    oledPush(fb);
  }
}
