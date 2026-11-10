// nixxa: DeskBunny faces on an ESP32 / ESP32-C3 / ESP32-S3, driven by a phone
// over BLE. Animated face (the DeskBunny engine, unmodified) on a 128x64
// SSD1306 over I2C, a library of 16 face slots in LittleFS, phone
// notifications and calls shown as cards. On an iPhone the device is also an
// ANCS client and reads the notifications itself.
// Contract: docs/ARCHITECTURE.md, "ESP32 + BLE (nixxa)" and "iOS (phase 3)".
//
// The face engine (deskbunny_engine.c/.h) lives in engine/; firmware/build.sh
// copies it next to this sketch before compiling. To build by hand, copy
// engine/deskbunny_engine.c and engine/deskbunny_engine.h into this folder.
//
// Tasks:
//   loop()      (Arduino loop task): serial, frame clock, drawing, I2C push.
//   "nx-proto"  protocol task: BLE messages, storage, pairing state, the ANCS
//               client. Owns LittleFS, NVS and every BLE call except the
//               stack's callbacks.
//   BLE host    stack callbacks: they only copy data and post events.
// Shared state lives in `S` and is guarded by `lock` (held for copies only,
// never across I/O).

#include <Arduino.h>
#include <esp_random.h>
#include <esp_system.h>
#include <string.h>

#include "deskbunny_engine.h"
#include "nx_ancs.h"
#include "nx_clock.h"
#include "nx_ble.h"
#include "nx_core.h"
#include "nx_display.h"
#include "nx_store.h"
#include "nx_version.h"

#if CONFIG_IDF_TARGET_ESP32
#define NX_BOARD "esp32"
#define NX_CHIP 0
#define NX_SDA 21
#define NX_SCL 22
#elif CONFIG_IDF_TARGET_ESP32C3
#define NX_BOARD "esp32c3"
#define NX_CHIP 1
#define NX_SDA 8
#define NX_SCL 9
#elif CONFIG_IDF_TARGET_ESP32S3
#define NX_BOARD "esp32s3"
#define NX_CHIP 2
#define NX_SDA 8
#define NX_SCL 9
#else
#error "nixxa supports the ESP32, ESP32-C3 and ESP32-S3"
#endif

// Second serial port: the USB CDC on C3/S3 is `Serial` with CDCOnBoot, UART0
// is `Serial0` then. On the classic ESP32, `Serial` is UART0.
#if ARDUINO_USB_CDC_ON_BOOT
#define NX_USB_SERIAL 1
#else
#define NX_USB_SERIAL 0
#endif

static const uint32_t FRAME_MS = 33;           // ~30 fps
static const uint32_t LOGO_MS = 1500;          // the app logo before its card
static const uint32_t CALL_MAX_MS = 45000;     // a call rings at most this long
static const uint32_t QR_SHOW_MS = 30000;      // serial 'Q'
static const uint32_t AUTH_AFTER_ENC_MS = 10000;
static const uint32_t AUTH_AFTER_CONN_MS = 30000; // pairing dialog on the phone
static const uint8_t CARD_QUEUE = 4;
static const uint8_t CARD_COLS = 21;           // 126 px of 5x7 text
static const uint8_t CARD_LINES_MAX = 64;
static const int BODY_TOP = 28, LINE_H = 9;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------
struct Card {
  uint32_t id;
  uint8_t mood;
  bool hasIcon;
  uint8_t icon[128]; // the app logo, 32x32 1 bpp (a 16x16 one is doubled on arrival)
  char app[19];   // 18 chars fit next to the icon
  char title[CARD_COLS + 1];
  char body[NX_BODY_MAX + 1];
  uint16_t starts[CARD_LINES_MAX];
  uint8_t lens[CARD_LINES_MAX];
  uint8_t lines;
};

struct Call {
  bool active;
  uint32_t id;
  uint32_t start;
  char caller[CARD_COLS + 1];
};

static struct {
  bool paired;
  bool connected, authed;
  uint8_t active, dnd;
  nx_notif_cfg_t cfg;
  // a new face/drawing for the renderer: staged here, swapped in by loop()
  bool faceDirty;
  uint8_t faceBlob[NX_FACE_SIZE];
  uint16_t artLen;
  char faceName[13];
  // notifications
  Card queue[CARD_QUEUE];
  uint8_t qHead, qCount;
  uint32_t shownId;      // card on screen (valid while shownActive)
  bool shownActive, dismissShown;
  Call call;
  uint32_t qrUntil;
  // wall clock from SET_TIME
  bool timeSet;
  uint32_t unixAt, millisAt;
  int16_t tzMin;
  // idle screen: 0 = the face, 1..NX_CLOCK_STYLES = that clock (SET_CLOCK)
  uint8_t clock;
} S;

static SemaphoreHandle_t lock;
#define LOCK() xSemaphoreTake(lock, portMAX_DELAY)
#define UNLOCK() xSemaphoreGive(lock)

// Drawing buffers: the renderer shows artA/artB[artCurIdx]; the other one is
// the stage the protocol task fills under the lock.
static uint8_t artBuf[2][NX_ART_MAX];
static uint8_t artCurIdx;

static uint8_t K[16];
static char devId[7];
static uint8_t devId3[3];
static char bleName[16];

// ---------------------------------------------------------------------------
// Serial (both ports on C3/S3)
// ---------------------------------------------------------------------------
static SemaphoreHandle_t outLock;

static void out(const char *s)
{
  xSemaphoreTake(outLock, portMAX_DELAY);
  Serial.print(s);
#if NX_USB_SERIAL
  Serial0.print(s);
#endif
  xSemaphoreGive(outLock);
}

static void outf(const char *fmt, ...)
{
  char buf[200];
  va_list ap;
  va_start(ap, fmt);
  vsnprintf(buf, sizeof(buf), fmt, ap);
  va_end(ap);
  out(buf);
}

static void printBanner()
{
  char name[13];
  LOCK();
  memcpy(name, S.faceName, sizeof(name));
  UNLOCK();
  outf("DESKBUNNY %s %s %s\r\n", NX_FW_VERSION, NX_BOARD, name);
}

// ---------------------------------------------------------------------------
// Notifications (called from the protocol task and from serial commands)
// ---------------------------------------------------------------------------
static uint16_t minuteOfDay(uint32_t now)
{
  int64_t local = (int64_t)S.unixAt + (int64_t)((uint32_t)(now - S.millisAt) / 1000u) + (int64_t)S.tzMin * 60;
  int64_t m = (local / 60) % 1440;
  if (m < 0) m += 1440;
  return (uint16_t)m;
}

static bool quietNow(uint32_t now)
{
  if (S.dnd) return true;
  return S.timeSet && nx_in_quiet(S.cfg.quiet_start, S.cfg.quiet_end, minuteOfDay(now));
}

// The logo as 32x32: a 16x16 icon from an older phone is pixel-doubled.
static void logoCopy(uint8_t dst[128], const uint8_t *src, uint8_t px)
{
  if (px == NX_ICON_BIG_PX) {
    memcpy(dst, src, 128);
    return;
  }
  memset(dst, 0, 128);
  for (int y = 0; y < 32; y++)
    for (int x = 0; x < 32; x++) {
      int sb = (y >> 1) * 16 + (x >> 1);
      if (src[sb >> 3] & (0x80u >> (sb & 7))) {
        int db = y * 32 + x;
        dst[db >> 3] |= (uint8_t)(0x80u >> (db & 7));
      }
    }
}

// Local wall-clock time from SET_TIME. Caller holds LOCK.
static nx_clock_time_t localClock(uint32_t now)
{
  int64_t local = (int64_t)S.unixAt + (int64_t)((uint32_t)(now - S.millisAt) / 1000u) + (int64_t)S.tzMin * 60;
  int64_t s = ((local % 86400) + 86400) % 86400;
  nx_clock_time_t t;
  t.hour = (uint8_t)(s / 3600);
  t.minute = (uint8_t)((s / 60) % 60);
  t.second = (uint8_t)(s % 60);
  return t;
}

// fromAncs: the device saw every notification itself (iOS), so a message is
// shown only when a rule names its app with "show"; NOTIFY (Android) was
// filtered on the phone and shows unless a rule hides it.
static void notifyApply(const nx_notify_t *n, bool fromAncs = false)
{
  static Card c; // built outside the lock; callers are serialised by cardLock
  static SemaphoreHandle_t cardLock = xSemaphoreCreateMutex();
  uint32_t now = millis();

  if (n->kind == 2) { // call ended
    LOCK();
    if (S.call.active && S.call.id == n->id) S.call.active = false;
    UNLOCK();
    return;
  }

  char app[NX_APPID_MAX + 1];
  size_t appLen = n->app_id_len > NX_APPID_MAX ? NX_APPID_MAX : n->app_id_len;
  memcpy(app, n->app_id, appLen);

  if (n->kind == 1) {
    Call call = {};
    call.active = true;
    call.id = n->id;
    call.start = now;
    if (n->title_len) nx_ascii(n->title, n->title_len, call.caller, sizeof(call.caller), 0);
    else nx_ascii(n->app_name, n->app_name_len, call.caller, sizeof(call.caller), 0);
    LOCK();
    bool show = S.cfg.enabled && (!quietNow(now) || S.cfg.calls_in_quiet);
    if (show) S.call = call;
    UNLOCK();
    return;
  }

  xSemaphoreTake(cardLock, portMAX_DELAY);
  LOCK();
  bool enabled = S.cfg.enabled, quiet = quietNow(now);
  const nx_rule_t *r = nx_cfg_find(&S.cfg, app, appLen);
  bool ruleHides = fromAncs ? !(r && (r->flags & 1)) : (r && !(r->flags & 1));
  uint8_t mood = nx_cfg_rule_mood(&S.cfg, r); // rule mood 0xFF = defaultMood
  UNLOCK();
  if (!enabled || quiet || ruleHides) {
    xSemaphoreGive(cardLock);
    return;
  }
  if (n->mood != 0xFF) mood = n->mood < DB_MOOD_COUNT ? n->mood : 0;

  memset(&c, 0, sizeof(c));
  c.id = n->id;
  c.mood = mood;
  c.hasIcon = n->icon != NULL;
  if (c.hasIcon) logoCopy(c.icon, n->icon, n->icon_px);
  nx_ascii(n->app_name, n->app_name_len, c.app, sizeof(c.app), 0);
  nx_ascii(n->title, n->title_len, c.title, sizeof(c.title), 0);
  nx_ascii(n->body, n->body_len, c.body, sizeof(c.body), 1);
  c.lines = (uint8_t)nx_wrap(c.body, CARD_COLS, c.starts, c.lens, CARD_LINES_MAX);

  LOCK();
  if (S.qCount == CARD_QUEUE) { // full: drop the oldest
    S.qHead = (uint8_t)((S.qHead + 1) % CARD_QUEUE);
    S.qCount--;
  }
  S.queue[(S.qHead + S.qCount) % CARD_QUEUE] = c;
  S.qCount++;
  UNLOCK();
  xSemaphoreGive(cardLock);
}

static void dismiss(uint32_t id, bool anyCall)
{
  LOCK();
  if (S.call.active && (anyCall || S.call.id == id)) S.call.active = false;
  if (!anyCall) {
    if (S.shownActive && S.shownId == id) S.dismissShown = true;
    uint8_t keep = 0;
    for (uint8_t i = 0; i < S.qCount; i++) {
      Card *src = &S.queue[(S.qHead + i) % CARD_QUEUE];
      if (src->id == id) continue;
      if (keep != i) S.queue[(S.qHead + keep) % CARD_QUEUE] = *src;
      keep++;
    }
    S.qCount = keep;
  }
  UNLOCK();
}

// ---------------------------------------------------------------------------
// Protocol task
// ---------------------------------------------------------------------------
enum { EV_MSG, EV_REPLY, EV_CONN, EV_DISC, EV_ENC, EV_UNPAIR, EV_ANCS };
struct Ev {
  uint8_t type, op, seq, arg;
  uint32_t len; // EV_MSG: bytes in rxBuf
};
static QueueHandle_t evq;
static TaskHandle_t protoHandle;

static uint8_t rxBuf[NX_MSG_MAX];
static nx_rx_t rx;
static volatile bool rxBusy; // rxBuf holds a message the protocol task hasn't finished
static uint8_t nonce[16];
static uint8_t ioFace[NX_FACE_SIZE];
static uint8_t ioArt[NX_ART_MAX];

static void post(uint8_t type, uint8_t op = 0, uint8_t seq = 0, uint8_t arg = 0, uint32_t len = 0)
{
  Ev e = { type, op, seq, arg, len };
  if (xQueueSend(evq, &e, 0) != pdTRUE) log_w("event queue full, dropped %u", type);
}

static void buildInfo()
{
  uint8_t info[NX_INFO_SIZE];
  nx_info_build(info, NX_CHIP, S.paired ? 1 : 0, devId3, nonce, NX_FW_VERSION);
  nxBleSetInfo(info, sizeof(info));
}

// BLE host context: copy, post, return.
void nxAppOnConnect()
{
  esp_fill_random(nonce, sizeof(nonce)); // fresh nonce per connection
  buildInfo();
  if (!rxBusy) nx_rx_init(&rx, rxBuf, sizeof(rxBuf)); // else the protocol task resets it
  post(EV_CONN);
}

void nxAppOnDisconnect() { post(EV_DISC); }
void nxAppOnEncrypted(bool ok) { post(EV_ENC, 0, 0, ok ? 1 : 0); }

// ANCS input (READY / NS / DS / CP results) is copied into its own queue, so
// a Data Source answer split over several notifications stays in order.
#define ANCS_PKT_MAX 512 // largest ATT value
struct AncsPkt {
  uint8_t what, status;
  uint16_t len;
  uint8_t d[ANCS_PKT_MAX];
};
static QueueHandle_t ancsq;

void nxAppOnAncs(uint8_t what, uint8_t status, const uint8_t *data, size_t len)
{
  static AncsPkt pk; // one BLE host task calls this: keep 0.5 KB off its stack
  pk.what = what;
  pk.status = status;
  pk.len = (uint16_t)(len > ANCS_PKT_MAX ? ANCS_PKT_MAX : len);
  if (pk.len) memcpy(pk.d, data, pk.len);
  if (xQueueSend(ancsq, &pk, 0) != pdTRUE) {
    log_w("ANCS queue full, dropped %u", what); // a lost answer times out
    return;
  }
  post(EV_ANCS);
}

void nxAppOnRx(const uint8_t *data, size_t len)
{
  if (rxBusy) { // the phone didn't wait for our reply
    if (len >= 4) post(EV_REPLY, data[0], data[1], NX_BUSY);
    return;
  }
  uint8_t op = 0, seq = 0;
  int r = nx_rx_feed(&rx, data, len, millis(), &op, &seq);
  if (r == NX_RX_MSG) {
    rxBusy = true;
    post(EV_MSG, op, seq, 0, rx.have);
  } else if (r == NX_RX_TOOBIG) {
    post(EV_REPLY, op, seq, NX_BAD_MSG);
  }
}

// A reply goes out as consecutive notifications of up to MTU-3 bytes.
struct TxStream {
  uint8_t buf[512];
  size_t n, cap;
  bool ok;
  void begin()
  {
    n = 0;
    cap = nxBleChunk();
    ok = nxBleConnected();
  }
  void put(const uint8_t *d, size_t len)
  {
    while (len && ok) {
      size_t k = cap - n < len ? cap - n : len;
      memcpy(buf + n, d, k);
      n += k;
      d += k;
      len -= k;
      if (n == cap) {
        ok = nxBleNotify(buf, n);
        n = 0;
      }
    }
  }
  void end()
  {
    if (n && ok) ok = nxBleNotify(buf, n);
    n = 0;
  }
};
static TxStream tx;

static void reply(uint8_t op, uint8_t seq, uint8_t status, const uint8_t *a = NULL, size_t an = 0,
                  const uint8_t *b = NULL, size_t bn = 0, const uint8_t *c = NULL, size_t cn = 0)
{
  uint8_t hdr[5];
  size_t len = 1 + an + bn + cn;
  hdr[0] = (uint8_t)(op | 0x80);
  hdr[1] = seq;
  nx_put16(hdr + 2, (uint16_t)len);
  hdr[4] = status;
  tx.begin();
  tx.put(hdr, 5);
  if (an) tx.put(a, an);
  if (bn) tx.put(b, bn);
  if (cn) tx.put(c, cn);
  tx.end();
  if (!tx.ok) log_w("reply 0x%02x not delivered", op);
}

// ---------------------------------------------------------------------------
// ANCS client (protocol task)
// ---------------------------------------------------------------------------
// The client and the seen-apps list (~4.5 KB) come from the heap once at boot:
// the classic ESP32's static DRAM segment is full with Bluedroid linked in.
struct AncsMem {
  nx_ancs_t client;
  nx_lru_t seen; // RAM only: LIST_SEEN_APPS, cleared on unpair
};
static AncsMem *am;       // NULL: no RAM for ANCS (then it stays off)
static uint8_t ancsLink;  // 0 not tried, 1 looking, 2 on, 3 absent (this connection)

static int ancsCpWrite(void *, const uint8_t *d, size_t n) { return nxBleAncsWriteCp(d, n) ? 0 : 1; }

static void ancsNote(void *, const nx_ancs_note_t *a)
{
  nx_notify_t n = {};
  n.id = a->uid;
  n.mood = 0xFF; // rule or default mood
  bool call = a->category == NX_ANCS_CAT_INCOMING_CALL;
  if (a->type == NX_ANCS_NOTE_REMOVED) {
    if (call) {
      n.kind = 2; // call ended (answered, declined or missed)
      notifyApply(&n, true);
    } else {
      dismiss(a->uid, false);
    }
    return;
  }
  if (call) {
    LOCK();
    bool forward = S.cfg.ext & NX_EXT_CALLS;
    UNLOCK();
    if (!forward) return;
  }
  n.kind = call ? 1 : 0;
  n.app_id = a->app_id;
  n.app_id_len = a->app_id_len;
  n.app_name = a->app_name;
  n.app_name_len = a->app_name_len;
  n.title = a->title; // the caller, for a call
  n.title_len = a->title_len;
  n.body = a->msg;
  n.body_len = a->msg_len;
  notifyApply(&n, true);
}

static void ancsInit()
{
  static const nx_ancs_ops_t ops = { ancsCpWrite, ancsNote, NULL };
  am = (AncsMem *)calloc(1, sizeof(AncsMem)); // once, at boot
  if (!am) return;
  nx_lru_clear(&am->seen);
  nx_ancs_init(&am->client, &ops, &am->seen);
}

static void ancsStop()
{
  ancsLink = 0;
  if (am) nx_ancs_stop(&am->client);
  xQueueReset(ancsq); // leftovers of the previous link
}

// Connection state, protocol task only.
static bool connActive, authed, encrypted;
static uint32_t connAt, encAt;
static int bondsAtConnect; // 0: open advertising, anyone could have bonded

static void stageFace(const uint8_t face[NX_FACE_SIZE], const uint8_t *art, uint16_t artLen)
{
  db_face_t f;
  db_face_decode(face, &f);
  LOCK();
  memcpy(S.faceBlob, face, NX_FACE_SIZE);
  memcpy(artBuf[artCurIdx ^ 1], art, artLen);
  S.artLen = artLen;
  memcpy(S.faceName, f.name, sizeof(S.faceName));
  S.faceDirty = true;
  UNLOCK();
}

// Show slot `slot` (falls back to the default face when it can't be read).
static void loadSlot(uint8_t slot)
{
  uint16_t artLen = 0;
  if (!nxSlotRead(slot, ioFace, ioArt, &artLen)) {
    db_face_t d;
    db_face_default(&d);
    db_face_encode(&d, ioFace);
    artLen = 0;
  }
  stageFace(ioFace, ioArt, artLen);
}

static void saveState()
{
  static nx_notif_cfg_t cfg;
  uint8_t active, dnd;
  LOCK();
  active = S.active;
  dnd = S.dnd;
  cfg = S.cfg;
  UNLOCK();
  if (!nxStateSave(active, dnd, &cfg)) out("nixxa: could not save /state.bin\r\n");
}

static void setPaired(bool p)
{
  LOCK();
  S.paired = p;
  UNLOCK();
}

static void doUnpair()
{
  nxBleDisconnect();
  nxBleDeleteAllBonds();
  if (am) {
    nx_ancs_stop(&am->client);
    nx_lru_clear(&am->seen); // the next owner may not be this phone
  }
  esp_fill_random(K, sizeof(K));
  if (!nxNvsSaveK(K)) out("nixxa: could not save K\r\n");
  nxNvsSetPaired(false);
  char qr[NX_QR_TEXT_LEN + 1];
  nx_qr_text(devId, K, qr);
  LOCK();
  nxQrSet(qr);
  S.paired = false;
  S.authed = false;
  UNLOCK();
  authed = false;
  buildInfo();
  if (!nxBleConnected()) nxBleAdvertise(); // else: on the disconnect event
  out("nixxa: unpaired, new key, showing the QR\r\n");
}

static void handleMessage(uint8_t op, uint8_t seq, const uint8_t *p, size_t len)
{
  if (op != NX_OP_AUTH && !authed) {
    reply(op, seq, NX_NOT_AUTHED);
    return;
  }
  switch (op) {
    case NX_OP_AUTH: {
      if (len != 16) { reply(op, seq, NX_BAD_MSG); return; }
      uint8_t tag[16];
      nx_auth_tag(K, nonce, tag);
      if (!nx_ct_equal(tag, p, 16)) {
        reply(op, seq, NX_AUTH_FAILED);
        out("nixxa: AUTH failed, disconnecting\r\n");
        vTaskDelay(pdMS_TO_TICKS(150)); // let the reply leave
        nxBleDisconnect();               // the bond goes on the disconnect event
        return;
      }
      authed = true;
      bool wasPaired;
      LOCK();
      wasPaired = S.paired;
      S.authed = true;
      UNLOCK();
      nxBleKeepOnlyCurrentPeer(); // this bond is the owner (re-bonded owner included)
      if (!wasPaired) {
        nxNvsSetPaired(true);
        setPaired(true);
        buildInfo();
        out("nixxa: paired\r\n");
      }
      reply(op, seq, NX_OK);
      if (ancsLink == 0 && am) { // iPhone: become an ANCS client on this link
        ancsLink = 1;
        nxBleAncsStart();
      }
      return;
    }
    case NX_OP_GET_STATE: {
      uint8_t r[5];
      LOCK();
      r[0] = S.active;
      r[3] = S.cfg.enabled;
      r[4] = S.dnd;
      UNLOCK();
      nx_put16(r + 1, nxSlotsUsedMask());
      reply(op, seq, NX_OK, r, 5);
      return;
    }
    case NX_OP_SET_TIME: {
      if (len != 6) { reply(op, seq, NX_BAD_MSG); return; }
      LOCK();
      S.unixAt = nx_get32(p);
      S.millisAt = millis();
      S.tzMin = (int16_t)nx_get16(p + 4);
      S.timeSet = true;
      UNLOCK();
      reply(op, seq, NX_OK);
      return;
    }
    case NX_OP_LIST_SLOTS: {
      uint8_t r[1 + NX_SLOTS * 13];
      size_t n = 1;
      uint16_t used = nxSlotsUsedMask();
      r[0] = 0;
      for (uint8_t i = 0; i < NX_SLOTS; i++) {
        if (!(used & (1u << i))) continue;
        char name[13];
        nxSlotName(i, name);
        if (!name[0]) snprintf(name, sizeof(name), "SLOT %02u", (unsigned)i);
        r[n++] = i;
        memset(r + n, 0, 12);
        memcpy(r + n, name, strnlen(name, 12));
        n += 12;
        r[0]++;
      }
      reply(op, seq, NX_OK, r, n);
      return;
    }
    case NX_OP_PUT_SLOT: {
      nx_put_t put;
      int st = nx_put_parse(p, len, &put);
      db_face_t f;
      if (st == NX_OK && db_face_decode(put.face, &f) != 0) st = NX_BAD_MSG; // CRC
      if (st == NX_OK) st = nxSlotWrite(put.slot, put.face, put.art, put.art_len);
      if (st == NX_OK) {
        uint8_t active;
        LOCK();
        active = S.active;
        UNLOCK();
        if (put.slot == active) stageFace(put.face, put.art, put.art_len);
      }
      reply(op, seq, (uint8_t)st);
      return;
    }
    case NX_OP_GET_SLOT: {
      if (len != 1) { reply(op, seq, NX_BAD_MSG); return; }
      uint16_t artLen = 0;
      if (p[0] >= NX_SLOTS || !(nxSlotsUsedMask() & (1u << p[0])) || !nxSlotRead(p[0], ioFace, ioArt, &artLen)) {
        reply(op, seq, NX_BAD_SLOT);
        return;
      }
      uint8_t al[2];
      nx_put16(al, artLen);
      reply(op, seq, NX_OK, ioFace, NX_FACE_SIZE, al, 2, ioArt, artLen);
      return;
    }
    case NX_OP_DELETE_SLOT: {
      if (len != 1) { reply(op, seq, NX_BAD_MSG); return; }
      uint8_t active;
      LOCK();
      active = S.active;
      UNLOCK();
      if (p[0] >= NX_SLOTS || p[0] == active) { reply(op, seq, NX_BAD_SLOT); return; }
      reply(op, seq, nxSlotDelete(p[0]) ? NX_OK : NX_NO_SPACE);
      return;
    }
    case NX_OP_ACTIVATE: {
      if (len != 1) { reply(op, seq, NX_BAD_MSG); return; }
      if (p[0] >= NX_SLOTS || !(nxSlotsUsedMask() & (1u << p[0]))) { reply(op, seq, NX_BAD_SLOT); return; }
      loadSlot(p[0]);
      LOCK();
      S.active = p[0];
      UNLOCK();
      saveState();
      reply(op, seq, NX_OK);
      return;
    }
    case NX_OP_SET_NOTIF_CONFIG: {
      static nx_notif_cfg_t cfg;
      if (nx_cfg_parse(p, len, DB_MOOD_COUNT, &cfg) != NX_OK) { reply(op, seq, NX_BAD_MSG); return; }
      LOCK();
      S.cfg = cfg;
      UNLOCK();
      saveState();
      reply(op, seq, NX_OK);
      return;
    }
    case NX_OP_NOTIFY: {
      nx_notify_t n;
      if (nx_notify_parse(p, len, &n) != NX_OK) { reply(op, seq, NX_BAD_MSG); return; }
      notifyApply(&n);
      reply(op, seq, NX_OK);
      return;
    }
    case NX_OP_DISMISS: {
      if (len != 4) { reply(op, seq, NX_BAD_MSG); return; }
      dismiss(nx_get32(p), false);
      reply(op, seq, NX_OK);
      return;
    }
    case NX_OP_SET_DND: {
      if (len != 1) { reply(op, seq, NX_BAD_MSG); return; }
      LOCK();
      S.dnd = p[0] ? 1 : 0;
      UNLOCK();
      saveState();
      reply(op, seq, NX_OK);
      return;
    }
    case NX_OP_SET_CLOCK: {
      if (len != 1 || p[0] > NX_CLOCK_STYLES) { reply(op, seq, NX_BAD_MSG); return; }
      LOCK();
      S.clock = p[0];
      UNLOCK();
      if (!nxClockSave(p[0])) out("nixxa: could not save /clock.bin\r\n");
      reply(op, seq, NX_OK);
      return;
    }
    case NX_OP_LIST_SEEN_APPS: {
      if (len != 0) { reply(op, seq, NX_BAD_MSG); return; }
      static_assert(NX_SEEN_APPS_MAX <= sizeof(ioArt), "ioArt holds the LIST_SEEN_APPS reply");
      size_t n = 1;
      ioArt[0] = 0;
      if (am) n = nx_lru_encode(&am->seen, ioArt); // ioArt: free outside slot I/O (this task)
      reply(op, seq, NX_OK, ioArt, n);
      return;
    }
    case NX_OP_UNPAIR: {
      reply(op, seq, NX_OK);
      vTaskDelay(pdMS_TO_TICKS(150));
      doUnpair();
      return;
    }
    default:
      reply(op, seq, NX_BAD_MSG);
      return;
  }
}

static void ancsDrain()
{
  static AncsPkt pk;
  while (xQueueReceive(ancsq, &pk, 0) == pdTRUE) {
    uint32_t now = millis();
    if (!am) continue;
    nx_ancs_t *ancs = &am->client;
    switch (pk.what) {
      case NX_ANCS_IN_READY:
        if (!connActive || !authed || ancsLink != 1) break; // stale
        ancsLink = pk.status ? 2 : 3;
        if (pk.status) nx_ancs_start(ancs);
        out(pk.status ? "nixxa: ANCS on (iPhone notifications)\r\n" : "nixxa: no ANCS on this link\r\n");
        break;
      case NX_ANCS_IN_NS: nx_ancs_on_ns(ancs, pk.d, pk.len, now); break;
      case NX_ANCS_IN_DS: nx_ancs_on_ds(ancs, pk.d, pk.len, now); break;
      case NX_ANCS_IN_CP:
        if (pk.status) log_w("ANCS control point error 0x%02x", pk.status);
        nx_ancs_cp_result(ancs, pk.status, now);
        break;
    }
  }
}

static void protoTask(void *)
{
  for (;;) {
    Ev e;
    if (xQueueReceive(evq, &e, pdMS_TO_TICKS(100)) == pdTRUE) {
      uint32_t now = millis();
      switch (e.type) {
        case EV_CONN:
          connActive = true;
          authed = encrypted = false;
          ancsStop();
          connAt = now;
          bondsAtConnect = nxBleBondCount();
          nxBleSnapshotBonds();
          LOCK();
          S.connected = true;
          S.authed = false;
          UNLOCK();
          break;
        case EV_ENC:
          if (e.arg) {
            encrypted = true;
            encAt = now;
          }
          break;
        case EV_DISC: {
          bool paired;
          LOCK();
          paired = S.paired;
          S.connected = false;
          S.authed = false;
          UNLOCK();
          // A phone that never authenticated leaves no bond behind: with no
          // owner bond (or unpaired) everything goes, otherwise only the bonds
          // made during this connection, so the owner's bond survives a
          // stranger or a failed AUTH from the owner.
          if (connActive && !authed) {
            if (!paired || bondsAtConnect == 0) nxBleDeleteAllBonds();
            else nxBleDropNewBonds();
            vTaskDelay(pdMS_TO_TICKS(100)); // Bluedroid deletes asynchronously
          }
          connActive = authed = encrypted = false;
          ancsStop();
          nxBleAdvertise();
          break;
        }
        case EV_MSG: {
          if (e.len >= NX_MSG_HDR) handleMessage(e.op, e.seq, rxBuf + NX_MSG_HDR, e.len - NX_MSG_HDR);
          nx_rx_reset(&rx);
          __sync_synchronize();
          rxBusy = false;
          break;
        }
        case EV_REPLY:
          reply(e.op, e.seq, e.arg);
          break;
        case EV_UNPAIR:
          doUnpair();
          break;
        case EV_ANCS:
          break; // drained below
      }
    }
    ancsDrain();
    uint32_t tnow = millis();
    if (am) nx_ancs_tick(&am->client, tnow);
    nxBleAdvTick(tnow);
    // AUTH deadline: 10 s once the link is encrypted (30 s to get there).
    if (connActive && !authed) {
      uint32_t now = millis();
      bool late = encrypted ? (uint32_t)(now - encAt) > AUTH_AFTER_ENC_MS : (uint32_t)(now - connAt) > AUTH_AFTER_CONN_MS;
      if (late) {
        out("nixxa: no AUTH in time, disconnecting\r\n");
        nxBleDisconnect();
        connAt = encAt = now; // don't repeat every 100 ms while the link drops
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Rendering (loop task)
// ---------------------------------------------------------------------------
static uint8_t fb[DB_FB_SIZE];
static db_engine_t engine;
static db_face_t face; // as decoded, including invert/rotate180
static const uint8_t *artPtr;
static uint16_t artSize;
static uint8_t artMode;
static uint16_t artFrames, artMs;
static uint16_t artIdx = 0xFFFF, artPos, artNext;

enum Screen { SCR_NONE, SCR_FACE, SCR_QR, SCR_LOGO, SCR_CARD, SCR_CALL, SCR_CLOCK };
static Screen lastScreen = SCR_NONE;
static Card cur;
static bool curActive;
static uint32_t curStart;
static Call call;
static uint8_t cfgContrast = 0xFF;
static int8_t cfgInvert = -1, cfgRotate = -1;

static uint8_t artRead(uint16_t off) { return artPtr[off]; }

static void artLoad()
{
  static const uint8_t magic[8] = { 'D', 'B', 'A', 'R', 'T', '0', '0', '1' };
  artMode = DB_ART_OFF;
  artIdx = 0xFFFF;
  if (artSize < DB_ART_HEADER || memcmp(artPtr, magic, 8) != 0 || artPtr[8] != DB_ART_VERSION) return;
  uint8_t mode = artPtr[9];
  uint16_t frames = nx_get16(artPtr + 10);
  if ((mode != DB_ART_REPLACE && mode != DB_ART_OVER) || frames < 1 || frames > DB_ART_FRAMES_MAX) return;
  uint16_t ms = nx_get16(artPtr + 12);
  if (ms == 0) ms = 500;
  artMs = ms < 50 ? 50 : (ms > 10000 ? 10000 : ms);
  artFrames = frames;
  artMode = mode;
}

static bool artDecode(uint8_t op)
{
  uint16_t p = artPos;
  if (db_art_frame(artRead, artSize, &p, op, fb) != 0) {
    artMode = DB_ART_OFF; // damaged drawing: the face shows instead
    return false;
  }
  artNext = p;
  return true;
}

// Same compositing as the AVR firmware (Drawing blob v2): replace mode keeps
// the decoded frame in fb and XORs each delta in; over mode ORs a whole frame
// onto the face.
static void artCompose(uint32_t now)
{
  uint16_t target = (uint16_t)((now / artMs) % artFrames);
  bool replace = artMode == DB_ART_REPLACE;
  uint8_t op = replace ? DB_ART_XOR : DB_ART_SKIP;
  if (target < artIdx) {
    artIdx = 0;
    artPos = DB_ART_HEADER;
    if (replace) memset(fb, 0, sizeof(fb));
    if (!artDecode(op)) return;
  }
  while (artIdx < target) {
    artPos = artNext;
    artIdx++;
    if (!artDecode(op)) return;
  }
  if (!replace) artDecode(DB_ART_OR);
}

static void engineFor(db_engine_t *e, const db_face_t *f, int mood)
{
  db_face_t d = *f;
  d.flags &= (uint8_t)~(DB_FLAG_INVERT | DB_FLAG_ROTATE180); // the panel does these
  if (mood >= 0) {
    d.mood = (uint8_t)mood;
    d.cycle = DB_CYCLE_OFF;
    d.overlay = DB_OVERLAY_NONE;
  }
  db_engine_init(e, &d, esp_random());
}

static void panel(bool faceScreen)
{
  uint8_t contrast = face.contrast;
  int8_t inv = faceScreen && (face.flags & DB_FLAG_INVERT) ? 1 : 0;
  int8_t rot = (face.flags & DB_FLAG_ROTATE180) ? 1 : 0;
  if (contrast == cfgContrast && inv == cfgInvert && rot == cfgRotate) return;
  nxDisplayConfig(contrast, inv, rot);
  cfgContrast = contrast;
  cfgInvert = inv;
  cfgRotate = rot;
}

static void drawQrScreen()
{
  nxFbClear(fb);
  LOCK();
  nxQrDraw(fb);
  UNLOCK();
  const char *lines[3] = { "NIXXA", devId, "SCAN ME" };
  for (int i = 0; i < 3; i++) {
    size_t n = strlen(lines[i]);
    nxText(fb, 64 + (64 - nxTextWidth(n)) / 2, 12 + i * 16, lines[i], n);
  }
}

// The logo stage: the app's logo centred, its name under it (replaces the face).
static void drawLogo()
{
  nxFbClear(fb);
  uint8_t big[128];
  if (cur.hasIcon) logoCopy(big, cur.icon, NX_ICON_BIG_PX);
  else logoCopy(big, NX_ICON_BELL, NX_ICON_SMALL_PX); // no logo: the bell, doubled
  nxIcon32(fb, (128 - 32) / 2, 2, big);
  nxTextCentered(fb, 42, cur.app, false);
}

// The notification card: a 16x16 version of the logo, the app name, a rule,
// the title and the scrolling body.
static void drawCard(uint32_t t)
{
  nxFbClear(fb);
  uint8_t small[32];
  if (cur.hasIcon) {
    memset(small, 0, sizeof(small));
    for (int y = 0; y < 16; y++)
      for (int x = 0; x < 16; x++) {
        bool on = false;
        for (int dy = 0; dy < 2 && !on; dy++)
          for (int dx = 0; dx < 2 && !on; dx++) {
            int b = (2 * y + dy) * 32 + (2 * x + dx);
            on = cur.icon[b >> 3] & (0x80u >> (b & 7));
          }
        if (on) small[(y * 16 + x) >> 3] |= (uint8_t)(0x80u >> ((y * 16 + x) & 7));
      }
  } else {
    memcpy(small, NX_ICON_BELL, sizeof(small));
  }
  nxIcon16(fb, 0, 0, small);
  nxText(fb, 20, 4, cur.app, strlen(cur.app));
  nxFbHLine(fb, 0, 127, 17);
  nxText(fb, 0, 19, cur.title, strlen(cur.title));
  int total = cur.lines * LINE_H, view = 64 - BODY_TOP;
  int off = 0;
  if (total > view) { // scroll: hold 1.5 s, 1 px per 40 ms, hold at the end
    int maxOff = total - view;
    off = t > 1500 ? (int)((t - 1500) / 40) : 0;
    if (off > maxOff) off = maxOff;
  }
  for (uint8_t i = 0; i < cur.lines; i++) {
    int y = BODY_TOP + i * LINE_H - off;
    nxText(fb, 0, y, cur.body + cur.starts[i], cur.lens[i], BODY_TOP, 64);
  }
}

static void drawCall(uint32_t t)
{
  nxFbClear(fb);
  bool ring = (t % 1000) < 600;
  int shake = ring ? (((t / 60) & 1) ? 2 : -2) : 0;
  nxIcon16(fb, 56 + shake, 2, NX_ICON_PHONE);
  if (ring) {
    nxText(fb, 36, 6, "((", 2);
    nxText(fb, 81, 6, "))", 2);
  }
  nxTextCentered(fb, 22, "INCOMING CALL", false);
  size_t n = strlen(call.caller);
  if (n * 9 <= 129) nxTextCentered(fb, 36, call.caller, true);
  else nxTextCentered(fb, 40, call.caller, false);
}

static void renderFrame(uint32_t now)
{
  bool paired;
  bool faceChanged = false;
  uint8_t clockStyle = 0;
  bool haveTime = false;
  nx_clock_time_t tm = { 0, 0, 0 };
  LOCK();
  if (S.faceDirty) {
    S.faceDirty = false;
    artCurIdx ^= 1;
    artPtr = artBuf[artCurIdx];
    artSize = S.artLen;
    db_face_decode(S.faceBlob, &face);
    faceChanged = true;
  }
  if (S.call.active && (uint32_t)(now - S.call.start) > CALL_MAX_MS) S.call.active = false;
  call = S.call;
  if (curActive && S.dismissShown) curActive = false;
  S.dismissShown = false;
  if (call.active) {
    if (curActive) curStart = now; // the card waits for the call to end
  } else {
    uint32_t cardMs = (uint32_t)S.cfg.card_secs * 1000u;
    if (curActive && (uint32_t)(now - curStart) >= LOGO_MS + cardMs) curActive = false;
    if (!curActive && S.qCount) {
      cur = S.queue[S.qHead];
      S.qHead = (uint8_t)((S.qHead + 1) % CARD_QUEUE);
      S.qCount--;
      curActive = true;
      curStart = now;
    }
  }
  S.shownActive = curActive;
  S.shownId = cur.id;
  paired = S.paired;
  bool qr = !paired || (int32_t)(S.qrUntil - now) > 0;
  clockStyle = S.clock;
  haveTime = S.timeSet;
  if (haveTime) tm = localClock(now);
  UNLOCK();

  if (faceChanged) {
    engineFor(&engine, &face, -1);
    artLoad();
  }

  Screen scr;
  if (call.active) scr = SCR_CALL;
  else if (curActive) scr = (uint32_t)(now - curStart) < LOGO_MS ? SCR_LOGO : SCR_CARD;
  else if (qr) scr = SCR_QR;
  else if (clockStyle) scr = SCR_CLOCK;
  else scr = SCR_FACE;

  if (scr != SCR_FACE) artIdx = 0xFFFF; // fb gets overwritten: replace mode restarts at frame 0
  panel(scr == SCR_FACE);
  switch (scr) {
    case SCR_FACE:
      if (artMode != DB_ART_REPLACE) db_engine_render(&engine, now, fb);
      if (artMode != DB_ART_OFF) artCompose(now);
      break;
    case SCR_LOGO: drawLogo(); break;
    case SCR_CARD: drawCard(now - curStart - LOGO_MS); break;
    case SCR_CLOCK: nx_clock_draw(fb, clockStyle, &tm, haveTime); break;
    case SCR_CALL: drawCall(now - call.start); break;
    case SCR_QR: drawQrScreen(); break;
    default: break;
  }
  lastScreen = scr;
  nxDisplayPush(fb);
}

// ---------------------------------------------------------------------------
// Serial commands: ? N C D Q U S
// ---------------------------------------------------------------------------
static uint32_t fakeId = 0x4E000000u;

static void fakeNotify(uint8_t kind, const char *app, const char *title, const char *body)
{
  nx_notify_t n = {};
  n.kind = kind;
  n.id = ++fakeId;
  n.mood = 0xFF;
  n.app_id = (const uint8_t *)app;
  n.app_id_len = (uint8_t)strnlen(app, 255);
  n.app_name = n.app_id;
  n.app_name_len = n.app_id_len;
  n.title = (const uint8_t *)title;
  n.title_len = (uint8_t)strnlen(title, 255);
  n.body = (const uint8_t *)body;
  size_t bl = strlen(body);
  n.body_len = (uint16_t)(bl > NX_BODY_MAX ? NX_BODY_MAX : bl);
  notifyApply(&n);
}

static void printState()
{
  bool paired, conn, auth;
  uint8_t active, dnd, q;
  bool call_;
  LOCK();
  paired = S.paired;
  conn = S.connected;
  auth = S.authed;
  active = S.active;
  dnd = S.dnd;
  q = S.qCount;
  call_ = S.call.active;
  UNLOCK();
  outf("nixxa: paired=%d active=%02u used=0x%04x ble=%s authed=%d bonds=%d dnd=%d cards=%u call=%d\r\n", paired,
       active, nxSlotsUsedMask(), conn ? "connected" : "advertising", auth, nxBleBondCount(), dnd, q, call_);
  outf("nixxa: id=%s name=%s stack=%s oled=0x%02X@%luHz fw=%s heap=%lu\r\n", devId, bleName, nxBleStack(),
       nxDisplayAddr(), (unsigned long)nxDisplayClock(), NX_FW_VERSION, (unsigned long)esp_get_free_heap_size());
  // Unused stack (bytes on ESP-IDF FreeRTOS) of the two app tasks.
  static const char *const ancsNames[4] = { "-", "looking", "on", "absent" };
  if (am)
    outf("nixxa: ancs=%s shown=%u failed=%u seen_apps=%u\r\n", ancsNames[ancsLink & 3], am->client.n_shown,
         am->client.n_failed, am->seen.n);
  else
    out("nixxa: ancs=no RAM\r\n");
  outf("nixxa: clock=%u (%s)\r\n", S.clock, S.clock ? nx_clock_name(S.clock) : "face");
  outf("nixxa: stack free loop=%u proto=%u\r\n", (unsigned)uxTaskGetStackHighWaterMark(NULL),
       protoHandle ? (unsigned)uxTaskGetStackHighWaterMark(protoHandle) : 0u);
}

static void command(char *line)
{
  size_t n = strlen(line);
  while (n && (line[n - 1] == ' ')) line[--n] = '\0';
  if (n == 0) return;
  char c = line[0];
  char *arg = line + 1;
  while (*arg == ' ') arg++;
  if (c == '?' && n == 1) {
    printBanner();
  } else if (c == 'N') {
    char *title = strchr(arg, '|');
    char *body = title ? strchr(title + 1, '|') : NULL;
    if (title) *title++ = '\0';
    if (body) *body++ = '\0';
    fakeNotify(0, arg, title ? title : "", body ? body : "");
    out("nixxa: notification queued\r\n");
  } else if (c == 'C') {
    fakeNotify(1, "phone", arg, "");
    out("nixxa: ringing\r\n");
  } else if (c == 'D' && n == 1) {
    dismiss(0, true);
    out("nixxa: call dismissed\r\n");
  } else if (c == 'Q' && n == 1) {
    LOCK();
    S.qrUntil = millis() + QR_SHOW_MS;
    UNLOCK();
    out("nixxa: QR for 30 s\r\n");
  } else if (c == 'K') { // bench: K 0 = face, K 1..NX_CLOCK_STYLES = that clock
    long v = atol(arg);
    if (v < 0 || v > NX_CLOCK_STYLES) {
      out("nixxa: K 0..20 (0 = face)\r\n");
    } else {
      LOCK();
      S.clock = (uint8_t)v;
      UNLOCK();
      nxClockSave((uint8_t)v);
      outf("nixxa: clock %ld %s\r\n", v, v ? nx_clock_name((uint8_t)v) : "face");
    }
  } else if (c == 'U' && n == 1) {
    post(EV_UNPAIR);
  } else if (c == 'S' && n == 1) {
    printState();
  } else {
    out("nixxa: commands: ?  N <app>|<title>|<body>  C <caller>  D  K <0-20>  Q  U  S\r\n");
  }
}

struct LineReader {
  char buf[640];
  size_t n;
  bool overflow;
  void poll(Stream &s)
  {
    while (s.available() > 0) {
      int c = s.read();
      if (c == '\r' || c == '\n') {
        if (!overflow && n) {
          buf[n] = '\0';
          command(buf);
        }
        n = 0;
        overflow = false;
      } else if (n < sizeof(buf) - 1) {
        buf[n++] = (char)c;
      } else {
        overflow = true; // too long: ignore the whole line
      }
    }
  }
};
static LineReader lineMain;
#if NX_USB_SERIAL
static LineReader lineUart;
#endif

// ---------------------------------------------------------------------------
static void splash()
{
  nxFbClear(fb);
  nxTextCentered(fb, 25, "nixxa", true);
  nxDisplayPush(fb);
  delay(800);
}

void setup()
{
  outLock = xSemaphoreCreateMutex();
  lock = xSemaphoreCreateMutex();
  evq = xQueueCreate(16, sizeof(Ev));
  ancsq = xQueueCreate(6, sizeof(AncsPkt));
  ancsInit();
  if (!am) out("nixxa: no RAM for the ANCS client\r\n");
  Serial.begin(115200);
#if NX_USB_SERIAL
  Serial.setTxTimeoutMs(0); // USB CDC with no host attached: drop, don't block
  Serial0.begin(115200);
#endif

  // Library and state.
  bool fsOk = nxStoreBegin();
  nx_cfg_default(&S.cfg);
  S.active = 0;
  uint8_t active = 0, dnd = 0;
  if (fsOk && nxStateLoad(&active, &dnd, &S.cfg)) {
    S.active = active;
    S.dnd = dnd;
  }
  if (fsOk && !(nxSlotsUsedMask() & 1u)) { // first boot: slot 00 = default face, active
    db_face_t d;
    db_face_default(&d);
    db_face_encode(&d, ioFace);
    nxSlotWrite(0, ioFace, NULL, 0);
    if (!(nxSlotsUsedMask() & (1u << S.active))) S.active = 0;
    nxStateSave(S.active, S.dnd, &S.cfg);
  }
  loadSlot(S.active); // missing/corrupt -> default face
  uint8_t clockStyle = 0;
  if (fsOk && nxClockLoad(&clockStyle)) S.clock = clockStyle;

  // Panel.
  nxDisplayBegin(NX_SDA, NX_SCL);
  db_face_decode(S.faceBlob, &face);
  panel(false);
  splash();

  // Identity, BLE, secret.
  uint8_t mac[6];
  nxBleMac(mac);
  nx_device_id(mac, devId);
  memcpy(devId3, mac + 3, 3);
  snprintf(bleName, sizeof(bleName), "NIXXA-%s", devId);
  bool bleOk = nxBleBegin(bleName);
  if (!nxNvsLoadK(K)) {
    esp_fill_random(K, sizeof(K)); // the radio is on now: true hardware RNG
    nxNvsSaveK(K);
    nxNvsSetPaired(false);
  }
  S.paired = nxNvsPaired();
  if (!S.paired && bleOk) nxBleDeleteAllBonds();
  char qr[NX_QR_TEXT_LEN + 1];
  nx_qr_text(devId, K, qr);
  nxQrSet(qr);
  esp_fill_random(nonce, sizeof(nonce));
  if (bleOk) {
    buildInfo();
    nxBleAdvertise();
  }

  // 8 KB: the deepest path (NOTIFY -> notifyApply) keeps its big buffers static.
  if (xTaskCreate(protoTask, "nx-proto", 8192, NULL, 2, &protoHandle) != pdPASS) out("nixxa: no protocol task\r\n");

  printBanner();
  if (!fsOk) out("nixxa: LittleFS mount failed, library unavailable\r\n");
  if (!nxDisplayAddr()) out("nixxa: no SSD1306 at 0x3C/0x3D\r\n");
  if (!bleOk) out("nixxa: BLE init failed\r\n");
}

void loop()
{
  static uint32_t lastFrame = millis() - FRAME_MS;
  lineMain.poll(Serial);
#if NX_USB_SERIAL
  lineUart.poll(Serial0);
  // Greet a USB host once when it first opens the port.
  static bool announced = false;
  if (!announced && Serial) {
    announced = true;
    printBanner();
  }
#endif
  uint32_t now = millis();
  if ((uint32_t)(now - lastFrame) >= FRAME_MS) {
    lastFrame += FRAME_MS;
    if ((uint32_t)(now - lastFrame) >= FRAME_MS) lastFrame = now; // fell behind: resync
    renderFrame(now);
  } else {
    vTaskDelay(1); // yield: the protocol task and BLE share this core on the C3
  }
}
