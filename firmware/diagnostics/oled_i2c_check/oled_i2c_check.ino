// DeskBunny OLED I2C check: is the display wired, and at which address?
//
// Flash it (Arduino IDE, or arduino-cli), open the Serial Monitor at 115200
// baud and read the verdict. It repeats every 3 s, so you can fix a wire and
// watch the result change. When an SSD1306 answers at 0x3C or 0x3D it also
// draws a border and a checkerboard on the screen.
//
// Wiring (I2C): GND-GND, VDD-5V, SDA and SCK/SCL to the board's SDA/SCL:
// Uno/Nano/Pro Mini/Uno R4 A4/A5, Mega 20/21, Leonardo/Micro 2/3.
//
// This replaces the DeskBunny firmware: flash DeskBunny again from the app
// when you are done.

#include <Wire.h>

static const uint8_t PIN_RES = 8; // only 7-pin modules have RES; harmless otherwise

// Time (us) for a line to read HIGH after we stop pulling it low, with the
// board's own pull-up off. The display module's pull-ups do this in a few us;
// with nothing on the line it never rises. 0xFFFF: did not rise in 2 ms.
static uint16_t riseTime(uint8_t pin)
{
  pinMode(pin, OUTPUT);
  digitalWrite(pin, LOW);
  delayMicroseconds(50);
  pinMode(pin, INPUT);
  uint32_t t = micros();
  while (digitalRead(pin) == LOW) {
    if (micros() - t > 2000) return 0xFFFF;
  }
  return (uint16_t)(micros() - t);
}

// HIGH or LOW with the board's own pull-up on: LOW means something holds the
// line down (a short to GND, or a display powered the wrong way round).
static bool stuckLow(uint8_t pin)
{
  pinMode(pin, INPUT_PULLUP);
  delay(2);
  return digitalRead(pin) == LOW;
}

static bool checkLine(uint8_t pin, const __FlashStringHelper *name)
{
  Serial.print(F("  "));
  Serial.print(name);
  Serial.print(F(" (pin "));
  Serial.print(pin);
  Serial.print(F("): "));
  if (stuckLow(pin)) {
    Serial.println(F("STUCK LOW. Shorted to GND, or VDD/GND swapped on the display."));
    return false;
  }
  uint16_t us = riseTime(pin);
  if (us == 0xFFFF) {
    Serial.println(F("no pull-up. Wire not connected here, or the display has no power."));
    return false;
  }
  Serial.print(F("OK, pulled up (rises in "));
  Serial.print(us);
  Serial.println(F(" us)"));
  return true;
}

// --- minimal SSD1306 over Wire, only for the test pattern -----------------
static void oledCmd(uint8_t addr, uint8_t c)
{
  Wire.beginTransmission(addr);
  Wire.write(0x00); // control byte: command
  Wire.write(c);
  Wire.endTransmission();
}

static void oledTestPattern(uint8_t addr)
{
  static const uint8_t init[] = {
    0xAE, 0xD5, 0x80, 0xA8, 0x3F, 0xD3, 0x00, 0x40, 0x8D, 0x14, 0x20, 0x02,
    0xA1, 0xC8, 0xDA, 0x12, 0x81, 0xCF, 0xD9, 0xF1, 0xDB, 0x40, 0xA4, 0xA6, 0xAF
  };
  for (uint8_t i = 0; i < sizeof(init); i++) oledCmd(addr, init[i]);
  for (uint8_t page = 0; page < 8; page++) {
    oledCmd(addr, 0xB0 + page);
    oledCmd(addr, 0x00);
    oledCmd(addr, 0x10);
    for (uint8_t x = 0; x < 128; x += 16) {
      Wire.beginTransmission(addr);
      Wire.write(0x40); // control byte: data
      for (uint8_t i = 0; i < 16; i++) {
        uint8_t col = x + i;
        uint8_t b = ((col >> 3) + page) & 1 ? 0xF0 : 0x0F; // checkerboard
        if (col == 0 || col == 127) b = 0xFF;               // left/right border
        if (page == 0) b |= 0x01;                           // top border
        if (page == 7) b |= 0x80;                           // bottom border
        Wire.write(b);
      }
      Wire.endTransmission();
    }
  }
}

// ---------------------------------------------------------------------------
void setup()
{
  Serial.begin(115200);
  uint32_t t = millis();
  while (!Serial && millis() - t < 3000) {} // native-USB boards: wait for the monitor
  pinMode(PIN_RES, OUTPUT);
  digitalWrite(PIN_RES, LOW);
  delay(20);
  digitalWrite(PIN_RES, HIGH);
  delay(50);
}

void loop()
{
  Serial.println();
  Serial.println(F("=== DeskBunny OLED I2C check ==="));
  Serial.println(F("1. Wires"));
  bool sdaOk = checkLine(SDA, F("SDA"));
  bool sclOk = checkLine(SCL, F("SCL/SCK"));

  Serial.println(F("2. I2C scan"));
  Wire.begin();
  Wire.setClock(100000);
#if defined(WIRE_HAS_TIMEOUT)
  Wire.setWireTimeout(5000, true);
#endif
  uint8_t found[8];
  uint8_t count = 0;
  bool hung = false;
  for (uint8_t a = 1; a < 127; a++) {
    Wire.beginTransmission(a);
    uint8_t r = Wire.endTransmission();
    if (r == 0) {
      if (count < sizeof(found)) found[count] = a;
      count++;
    } else if (r == 5) {
      hung = true;
      break;
    }
  }

  uint8_t display = 0;
  if (hung) {
    Serial.println(F("  The bus hung (a line held low mid-transfer)."));
  } else if (count == 0) {
    Serial.println(F("  Nothing answered."));
  } else if (count > sizeof(found)) {
    Serial.print(F("  "));
    Serial.print(count);
    Serial.println(F(" addresses answered: that is not a real device, SDA is held low."));
  } else {
    Serial.print(F("  Answered:"));
    for (uint8_t i = 0; i < count; i++) {
      Serial.print(F(" 0x"));
      if (found[i] < 16) Serial.print('0');
      Serial.print(found[i], HEX);
      if (found[i] == 0x3C || found[i] == 0x3D) display = found[i];
    }
    Serial.println();
  }

  Serial.println(F("3. Verdict"));
  if (display) {
    Serial.print(F("  DISPLAY FOUND at 0x"));
    Serial.print(display, HEX);
    Serial.println(F(". Drawing a border + checkerboard on it now."));
    Serial.println(F("  Screen still blank? The panel itself is faulty, or it is not an SSD1306."));
    oledTestPattern(display);
  } else if (!sdaOk || !sclOk) {
    Serial.println(F("  NO DISPLAY: fix the wire(s) flagged in step 1 first."));
  } else if (hung || count > sizeof(found)) {
    Serial.println(F("  NO DISPLAY: the lines are powered but the bus misbehaves."));
    Serial.println(F("  Check VDD->5V and GND->GND (easy to swap), then SDA/SCK swapped."));
    Serial.println(F("  If the display is warm, unplug it: it may be damaged."));
  } else if (count == 0) {
    Serial.println(F("  NO DISPLAY: lines look fine but nothing answers."));
    Serial.println(F("  Try SDA and SCK swapped. Check the board's pins are SDA/SCL (A4/A5 on a Nano)."));
  } else {
    Serial.println(F("  Something answered, but not at 0x3C/0x3D: not an SSD1306 display."));
  }
  Wire.end();
  delay(3000);
}
