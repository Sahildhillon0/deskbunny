// Reference: the user's original, known-working sketch (Uno R4, bit-banged SPI SSD1306).
// Pins: CLK 13, MOSI 11, CS 10, DC 9, RES 8. Kept verbatim for the init sequence and timing.

const int PIN_OLED_CLK  = 13;
const int PIN_OLED_MOSI = 11;
const int PIN_OLED_CS   = 10;
const int PIN_OLED_DC   = 9;
const int PIN_OLED_RES  = 8;

void spiWriteByte(uint8_t data) {
  for (int i = 7; i >= 0; i--) {
    digitalWrite(PIN_OLED_CLK, LOW);
    digitalWrite(PIN_OLED_MOSI, (data & (1 << i)) ? HIGH : LOW);
    digitalWrite(PIN_OLED_CLK, HIGH);
  }
}

void oledWriteCommand(uint8_t cmd) {
  digitalWrite(PIN_OLED_CS, LOW);
  digitalWrite(PIN_OLED_DC, LOW);
  spiWriteByte(cmd);
  digitalWrite(PIN_OLED_CS, HIGH);
}

void oledWriteData(uint8_t data) {
  digitalWrite(PIN_OLED_CS, LOW);
  digitalWrite(PIN_OLED_DC, HIGH);
  spiWriteByte(data);
  digitalWrite(PIN_OLED_CS, HIGH);
}

void oledSetCursor(uint8_t page, uint8_t col) {
  oledWriteCommand(0xB0 + (page & 0x07));
  oledWriteCommand(0x00 | (col & 0x0F));
  oledWriteCommand(0x10 | ((col >> 4) & 0x0F));
}

void oledInit() {
  pinMode(PIN_OLED_CLK, OUTPUT);
  pinMode(PIN_OLED_MOSI, OUTPUT);
  pinMode(PIN_OLED_CS, OUTPUT);
  pinMode(PIN_OLED_DC, OUTPUT);
  pinMode(PIN_OLED_RES, OUTPUT);
  digitalWrite(PIN_OLED_CS, HIGH);
  digitalWrite(PIN_OLED_CLK, HIGH);
  digitalWrite(PIN_OLED_RES, HIGH); delay(10);
  digitalWrite(PIN_OLED_RES, LOW);  delay(50);
  digitalWrite(PIN_OLED_RES, HIGH); delay(50);
  oledWriteCommand(0xAE);
  oledWriteCommand(0xD5); oledWriteCommand(0x80);
  oledWriteCommand(0xA8); oledWriteCommand(0x3F);
  oledWriteCommand(0xD3); oledWriteCommand(0x00);
  oledWriteCommand(0x40);
  oledWriteCommand(0x8D); oledWriteCommand(0x14);   // charge pump on
  oledWriteCommand(0x20); oledWriteCommand(0x02);   // page addressing
  oledWriteCommand(0xA1);
  oledWriteCommand(0xC8);
  oledWriteCommand(0xDA); oledWriteCommand(0x12);
  oledWriteCommand(0x81); oledWriteCommand(0xCF);
  oledWriteCommand(0xD9); oledWriteCommand(0xF1);
  oledWriteCommand(0xDB); oledWriteCommand(0x40);
  oledWriteCommand(0xA4);
  oledWriteCommand(0xA6);
  oledWriteCommand(0xAF);
}

// The original also had a 5x7 font (ASCII 32..90), oledClearDisplay(), oledPrintChar()
// and oledPrintString(); setup() printed "HELLO NK!" and "UNO R4 SPI DRIVER".
