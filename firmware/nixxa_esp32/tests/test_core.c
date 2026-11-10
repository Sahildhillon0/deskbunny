/* Host tests for the nixxa protocol core (nx_core.c, nx_ancs.c, nx_sha256.c, qrcodegen.c).
 *   make -C firmware/nixxa_esp32/tests
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "../nx_core.h"
#include "../nx_ancs.h"
#include "../nx_sha256.h"
#include "../qrcodegen.h"

static int fails;
#define CHECK(c)                                                         \
  do {                                                                   \
    if (!(c)) {                                                          \
      fprintf(stderr, "%s:%d: CHECK failed: %s\n", __FILE__, __LINE__, #c); \
      fails++;                                                           \
    }                                                                    \
  } while (0)

static void hex(const uint8_t *d, size_t n, char *out)
{
  size_t i;
  for (i = 0; i < n; i++) sprintf(out + 2 * i, "%02x", d[i]);
}

static void test_sha_hmac(void)
{
  uint8_t out[32], key[131], msg[1000];
  char h[65];
  nx_sha256_t s;
  nx_sha256_init(&s);
  nx_sha256_update(&s, (const uint8_t *)"abc", 3);
  nx_sha256_final(&s, out);
  hex(out, 32, h);
  CHECK(strcmp(h, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad") == 0);

  memset(msg, 'a', sizeof(msg));
  nx_sha256_init(&s);
  nx_sha256_update(&s, msg, 3);
  nx_sha256_update(&s, msg, 997); /* split across blocks */
  nx_sha256_final(&s, out);
  hex(out, 32, h);
  CHECK(strcmp(h, "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3") == 0);

  memset(key, 0x0b, 20); /* RFC 4231 case 1 */
  nx_hmac_sha256(key, 20, (const uint8_t *)"Hi There", 8, out);
  hex(out, 32, h);
  CHECK(strcmp(h, "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7") == 0);

  memset(key, 0xaa, 131); /* RFC 4231 case 6: key longer than a block */
  {
    const char *m = "Test Using Larger Than Block-Size Key - Hash Key First";
    nx_hmac_sha256(key, 131, (const uint8_t *)m, strlen(m), out);
  }
  hex(out, 32, h);
  CHECK(strcmp(h, "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54") == 0);
}

static void test_auth(void)
{
  uint8_t k[16], nonce[16], tag[16], bad[16];
  char h[33];
  int i;
  for (i = 0; i < 16; i++) { k[i] = (uint8_t)i; nonce[i] = (uint8_t)(0x10 + i); }
  nx_auth_tag(k, nonce, tag);
  hex(tag, 16, h);
  /* python: hmac.new(bytes(range(16)), bytes(range(16,32)) + b"NIXXA-AUTH", sha256).hexdigest()[:32] */
  CHECK(strcmp(h, "8b0bfcfaf44904dcf18843ba7721e362") == 0);
  CHECK(nx_ct_equal(tag, tag, 16));
  memcpy(bad, tag, 16);
  bad[15] ^= 1;
  CHECK(!nx_ct_equal(tag, bad, 16));
}

static void test_ids_qr(void)
{
  const uint8_t mac[6] = { 0x24, 0x6F, 0x28, 0xA1, 0xB2, 0xC3 };
  uint8_t k[16];
  char id[7], qr[NX_QR_TEXT_LEN + 1];
  uint8_t tmp[qrcodegen_BUFFER_LEN_FOR_VERSION(3)], code[qrcodegen_BUFFER_LEN_FOR_VERSION(3)];
  struct qrcodegen_Segment seg;
  int i;
  nx_device_id(mac, id);
  CHECK(strcmp(id, "A1B2C3") == 0);
  for (i = 0; i < 16; i++) k[i] = (uint8_t)(0xF0 + i);
  nx_qr_text(id, k, qr);
  CHECK(strcmp(qr, "NIXXA1:A1B2C3:F0F1F2F3F4F5F6F7F8F9FAFBFCFDFEFF") == 0);
  CHECK(strlen(qr) == NX_QR_TEXT_LEN);
  CHECK(qrcodegen_isAlphanumeric(qr));
  /* Same call as the firmware: alphanumeric, ECC M, version <= 3. */
  CHECK(qrcodegen_calcSegmentBufferSize(qrcodegen_Mode_ALPHANUMERIC, strlen(qr)) <= sizeof(tmp));
  seg = qrcodegen_makeAlphanumeric(qr, tmp);
  CHECK(qrcodegen_encodeSegmentsAdvanced(&seg, 1, qrcodegen_Ecc_MEDIUM, 1, 3, qrcodegen_Mask_AUTO, false,
                                         tmp, code));
  CHECK(qrcodegen_getSize(code) == 29);
}

static void test_info(void)
{
  uint8_t info[NX_INFO_SIZE], nonce[16];
  const uint8_t id3[3] = { 0xA1, 0xB2, 0xC3 };
  int i;
  for (i = 0; i < 16; i++) nonce[i] = (uint8_t)(100 + i);
  nx_info_build(info, 2, 1, id3, nonce, "0.2.0");
  CHECK(info[0] == 1 && info[1] == 2 && info[2] == 1 && info[3] == 16);
  CHECK(nx_get16(info + 4) == 12352);
  CHECK(memcmp(info + 6, id3, 3) == 0);
  CHECK(memcmp(info + 9, nonce, 16) == 0);
  CHECK(memcmp(info + 25, "0.2.0", 5) == 0 && info[30] == 0 && info[40] == 0);
}

static void test_rx(void)
{
  static uint8_t buf[NX_MSG_MAX];
  nx_rx_t rx;
  uint8_t op = 0, seq = 0, msg[300];
  uint32_t t = 1000;
  int i;
  nx_rx_init(&rx, buf, sizeof(buf));

  /* one write, one message */
  {
    const uint8_t w[] = { 0x02, 7, 0, 0 };
    CHECK(nx_rx_feed(&rx, w, sizeof(w), t, &op, &seq) == NX_RX_MSG);
    CHECK(op == 0x02 && seq == 7 && rx.have == 4);
    nx_rx_reset(&rx);
  }
  /* split header and body across 4 writes, then trailing junk is dropped */
  for (i = 0; i < 300; i++) msg[i] = (uint8_t)i;
  msg[0] = 0x21; msg[1] = 9; nx_put16(msg + 2, 296);
  CHECK(nx_rx_feed(&rx, msg, 1, t += 5, &op, &seq) == NX_RX_MORE);
  CHECK(nx_rx_feed(&rx, msg + 1, 2, t += 5, &op, &seq) == NX_RX_MORE);
  CHECK(nx_rx_feed(&rx, msg + 3, 100, t += 5, &op, &seq) == NX_RX_MORE);
  CHECK(nx_rx_feed(&rx, msg + 103, 197, t += 5, &op, &seq) == NX_RX_MSG);
  CHECK(op == 0x21 && seq == 9 && rx.have == 300 && memcmp(buf, msg, 300) == 0);
  nx_rx_reset(&rx);

  /* a 300 ms pause abandons a half message */
  CHECK(nx_rx_feed(&rx, msg, 50, t += 5, &op, &seq) == NX_RX_MORE);
  {
    const uint8_t w[] = { 0x10, 1, 0, 0 };
    CHECK(nx_rx_feed(&rx, w, 4, t += 300, &op, &seq) == NX_RX_MSG);
    CHECK(op == 0x10 && seq == 1);
    nx_rx_reset(&rx);
  }

  /* oversize: BAD_MSG once, drop until a write after a pause */
  {
    uint8_t w[20] = { 0x11, 3 };
    nx_put16(w + 2, (uint16_t)(NX_MAX_PAYLOAD + 1));
    CHECK(nx_rx_feed(&rx, w, 20, t += 400, &op, &seq) == NX_RX_TOOBIG);
    CHECK(op == 0x11 && seq == 3);
    CHECK(nx_rx_feed(&rx, w, 20, t += 10, &op, &seq) == NX_RX_DROPPED);
    CHECK(nx_rx_feed(&rx, w, 20, t += 299, &op, &seq) == NX_RX_DROPPED);
    w[0] = 0x02; w[1] = 4; w[2] = 0; w[3] = 0;
    CHECK(nx_rx_feed(&rx, w, 4, t += 300, &op, &seq) == NX_RX_MSG);
    CHECK(op == 0x02 && seq == 4);
    nx_rx_reset(&rx);
  }
  /* exactly maxPayload fits */
  {
    uint8_t w[4] = { 0x11, 5 };
    nx_put16(w + 2, (uint16_t)NX_MAX_PAYLOAD);
    CHECK(nx_rx_feed(&rx, w, 4, t += 400, &op, &seq) == NX_RX_MORE);
    for (i = 0; i < (int)NX_MAX_PAYLOAD / 200; i++) CHECK(nx_rx_feed(&rx, msg, 200, t += 1, &op, &seq) == NX_RX_MORE);
    CHECK(nx_rx_feed(&rx, msg, NX_MAX_PAYLOAD % 200, t += 1, &op, &seq) == NX_RX_MSG);
    CHECK(rx.have == NX_MSG_MAX);
    nx_rx_reset(&rx);
  }
  /* time wrap */
  {
    const uint8_t w[] = { 0x02, 8, 0, 0 };
    CHECK(nx_rx_feed(&rx, w, 2, 0xFFFFFFF0u, &op, &seq) == NX_RX_MORE);
    CHECK(nx_rx_feed(&rx, w + 2, 2, 0x10u, &op, &seq) == NX_RX_MSG);
    nx_rx_reset(&rx);
  }
}

static size_t put_str8(uint8_t *p, const char *s)
{
  size_t n = strlen(s);
  p[0] = (uint8_t)n;
  memcpy(p + 1, s, n);
  return n + 1;
}

static void test_cfg(void)
{
  static nx_notif_cfg_t c, d;
  uint8_t p[NX_CFG_WIRE_MAX + 8], w[NX_CFG_WIRE_MAX];
  size_t n = 0, wl;
  const nx_rule_t *r;
  int i;
  nx_cfg_default(&c);
  CHECK(c.enabled == 1 && c.card_secs == 6 && c.rule_count == 0);
  p[n++] = 1; p[n++] = 99; p[n++] = 200;
  nx_put16(p + n, 22 * 60); n += 2;
  nx_put16(p + n, 7 * 60); n += 2;
  p[n++] = 1; p[n++] = 2;
  n += put_str8(p + n, "com.whatsapp"); p[n++] = 6; p[n++] = 1;
  n += put_str8(p + n, "com.slack"); p[n++] = 2; p[n++] = 0xFE;
  CHECK(nx_cfg_parse(p, n, 30, &c) == NX_OK);
  CHECK(c.card_secs == 30 && c.default_mood == 0 && c.quiet_start == 1320 && c.quiet_end == 420);
  CHECK(c.calls_in_quiet == 1 && c.rule_count == 2);
  r = nx_cfg_find(&c, "com.slack", 9);
  CHECK(r && r->mood == 2 && r->flags == 0);
  r = nx_cfg_find(&c, "com.whatsapp", 12);
  CHECK(r && r->mood == 6 && r->flags == 1);
  CHECK(nx_cfg_find(&c, "com.slac", 8) == NULL);
  /* round trip through the storage format */
  wl = nx_cfg_write(&c, w);
  CHECK(nx_cfg_parse(w, wl, 30, &d) == NX_OK);
  CHECK(memcmp(&c, &d, sizeof(c)) == 0);
  /* errors leave the config alone */
  CHECK(nx_cfg_parse(p, n - 1, 30, &d) == NX_BAD_MSG);
  CHECK(nx_cfg_parse(p, 8, 30, &d) == NX_BAD_MSG);
  p[8] = 33;
  CHECK(nx_cfg_parse(p, n, 30, &d) == NX_BAD_MSG);
  CHECK(memcmp(&c, &d, sizeof(c)) == 0);
  /* 32 rules of 64-byte ids = the largest config */
  n = 0;
  p[n++] = 1; p[n++] = 6; p[n++] = 1; nx_put16(p + n, 0); n += 2; nx_put16(p + n, 0); n += 2;
  p[n++] = 0; p[n++] = 32;
  for (i = 0; i < 32; i++) {
    p[n++] = 64; memset(p + n, 'a' + (i % 26), 64); p[n] = (uint8_t)('A' + i); n += 64;
    p[n++] = 1; p[n++] = 1;
  }
  CHECK(n + 1 == NX_CFG_WIRE_MAX); /* + the ext byte */
  CHECK(nx_cfg_parse(p, n, 30, &c) == NX_OK && c.rule_count == 32 && c.ext == NX_EXT_CALLS);
  p[n++] = 0;
  CHECK(nx_cfg_parse(p, n, 30, &c) == NX_OK && c.ext == 0);
  CHECK(nx_cfg_write(&c, w) == NX_CFG_WIRE_MAX);

  CHECK(!nx_in_quiet(0, 0, 100));
  CHECK(nx_in_quiet(60, 120, 60) && nx_in_quiet(60, 120, 119) && !nx_in_quiet(60, 120, 120));
  CHECK(nx_in_quiet(1320, 420, 1439) && nx_in_quiet(1320, 420, 0) && nx_in_quiet(1320, 420, 419));
  CHECK(!nx_in_quiet(1320, 420, 420) && !nx_in_quiet(1320, 420, 1319));
}

static void test_notify_put(void)
{
  uint8_t p[1200];
  size_t n = 0;
  nx_notify_t m;
  nx_put_t put;
  int i;
  p[n++] = 0; nx_put16(p + n, 0x5678); nx_put16(p + n + 2, 0x1234); n += 4; p[n++] = 0xFF;
  n += put_str8(p + n, "com.x");
  n += put_str8(p + n, "X");
  n += put_str8(p + n, "Hello");
  nx_put16(p + n, 600); n += 2;
  memset(p + n, 'b', 600); n += 600;
  p[n++] = 1;
  for (i = 0; i < 32; i++) p[n++] = (uint8_t)i;
  CHECK(nx_notify_parse(p, n, &m) == NX_OK);
  CHECK(m.kind == 0 && m.id == 0x12345678u && m.mood == 0xFF);
  CHECK(m.app_id_len == 5 && memcmp(m.app_id, "com.x", 5) == 0);
  CHECK(m.title_len == 5 && m.body_len == NX_BODY_MAX && m.icon && m.icon[31] == 31);
  CHECK(nx_notify_parse(p, n - 1, &m) == NX_BAD_MSG);
  p[0] = 3;
  CHECK(nx_notify_parse(p, n, &m) == NX_BAD_MSG);
  p[0] = 1;
  p[n - 33] = 0; /* hasIcon = 0 but icon bytes follow */
  CHECK(nx_notify_parse(p, n, &m) == NX_BAD_MSG);
  CHECK(nx_notify_parse(p, n - 32, &m) == NX_OK && m.icon == NULL && m.kind == 1);

  /* flag 2: a 32x32 logo (128 bytes); flag 3 is not a thing */
  {
    uint8_t q[1200];
    size_t k = 0;
    memset(q, 0, sizeof(q));
    q[k++] = 0; nx_put16(q + k, 0x5678); nx_put16(q + k + 2, 0x1234); k += 4; q[k++] = 0xFF;
    k += put_str8(q + k, "com.x");
    k += put_str8(q + k, "X");
    k += put_str8(q + k, "Hello");
    nx_put16(q + k, 2); k += 2; q[k++] = 'h'; q[k++] = 'i';
    q[k++] = 2;
    for (i = 0; i < 128; i++) q[k++] = (uint8_t)(i ^ 0x5A);
    CHECK(nx_notify_parse(q, k, &m) == NX_OK && m.icon_px == NX_ICON_BIG_PX && m.icon && m.icon[127] == (127 ^ 0x5A));
    CHECK(nx_notify_parse(q, k - 1, &m) == NX_BAD_MSG);
    q[k - 129] = 3;
    CHECK(nx_notify_parse(q, k, &m) == NX_BAD_MSG);
  }

  /* PUT_SLOT */
  memset(p, 0, sizeof(p));
  p[0] = 3;
  CHECK(nx_put_parse(p, 67, &put) == NX_OK && put.slot == 3 && put.art_len == 0 && put.art == NULL);
  CHECK(nx_put_parse(p, 66, &put) == NX_BAD_MSG);
  CHECK(nx_put_parse(p, 68, &put) == NX_BAD_MSG);
  p[0] = 16;
  CHECK(nx_put_parse(p, 67, &put) == NX_BAD_SLOT);
  p[0] = 0;
  nx_put16(p + 65, 20);
  memcpy(p + 67, "DBART001", 8); p[75] = 2; p[76] = 1; nx_put16(p + 77, 1);
  CHECK(nx_put_parse(p, 87, &put) == NX_OK && put.art_len == 20 && put.art == p + 67);
  p[75] = 1;
  CHECK(nx_put_parse(p, 87, &put) == NX_BAD_MSG);
  CHECK(1u + NX_FACE_SIZE + 2u + NX_ART_MAX == NX_MAX_PAYLOAD);
}

static void test_text(void)
{
  char out[64];
  uint16_t st[16];
  uint8_t ln[16];
  size_t n;
  /* "Caf\xC3\xA9 \xF0\x9F\x98\x80!\x01" -> "Caf? ?!?" */
  const uint8_t u[] = { 'C', 'a', 'f', 0xC3, 0xA9, ' ', 0xF0, 0x9F, 0x98, 0x80, '!', 0x01, '\n', 'x', '\r' };
  n = nx_ascii(u, sizeof(u), out, sizeof(out), 0);
  CHECK(strcmp(out, "Caf? ?!? x") == 0 && n == 10);
  nx_ascii(u, sizeof(u), out, sizeof(out), 1);
  CHECK(strcmp(out, "Caf? ?!?\nx") == 0);
  nx_ascii((const uint8_t *)"abcdef", 6, out, 4, 0);
  CHECK(strcmp(out, "abc") == 0);
  {
    const uint8_t stray[] = { 0x80, 'a', 0xFF };
    nx_ascii(stray, 3, out, sizeof(out), 0);
    CHECK(strcmp(out, "?a?") == 0);
  }

  n = nx_wrap("the quick brown fox jumps", 10, st, ln, 16);
  CHECK(n == 3);
  CHECK(ln[0] == 9 && memcmp("the quick brown fox jumps" + st[0], "the quick", 9) == 0);
  CHECK(ln[1] == 9 && memcmp("the quick brown fox jumps" + st[1], "brown fox", 9) == 0);
  CHECK(ln[2] == 5 && memcmp("the quick brown fox jumps" + st[2], "jumps", 5) == 0);
  n = nx_wrap("abcdefghijklmnopqrstuvwxy", 10, st, ln, 16);
  CHECK(n == 3 && ln[0] == 10 && ln[1] == 10 && ln[2] == 5 && st[2] == 20);
  n = nx_wrap("one\n\ntwo", 10, st, ln, 16);
  CHECK(n == 3 && ln[0] == 3 && ln[1] == 0 && ln[2] == 3);
  n = nx_wrap("exactly10c next", 10, st, ln, 16);
  CHECK(n == 2 && ln[0] == 10 && ln[1] == 4);
  n = nx_wrap("a b c d e f g h i j k l m n o p q r s t u v w x y z", 3, st, ln, 4);
  CHECK(n == 4);
  CHECK(nx_wrap("", 10, st, ln, 16) == 0);
  CHECK(nx_wrap("   ", 10, st, ln, 16) == 0);
}

/* ---- phase 3: SET_NOTIF_CONFIG ext byte, mood 0xFF ---------------------- */
static void test_cfg_ext_mood(void)
{
  static nx_notif_cfg_t c, d;
  uint8_t p[256], w[NX_CFG_WIRE_MAX];
  size_t n = 0, base, wl;
  const nx_rule_t *r;
  p[n++] = 1; p[n++] = 6; p[n++] = 3;           /* defaultMood 3 */
  nx_put16(p + n, 0); n += 2; nx_put16(p + n, 0); n += 2;
  p[n++] = 0; p[n++] = 3;
  n += put_str8(p + n, "com.apple.MobileSMS"); p[n++] = 0xFF; p[n++] = 1; /* defaultMood */
  n += put_str8(p + n, "net.whatsapp.WhatsApp"); p[n++] = 7; p[n++] = 1;
  n += put_str8(p + n, "com.bad"); p[n++] = 0xFE; p[n++] = 1;           /* unknown -> 0 */
  base = n;

  /* old payload (no ext): calls forwarded by default */
  CHECK(nx_cfg_parse(p, base, 30, &c) == NX_OK);
  CHECK(c.ext == NX_EXT_CALLS);
  r = nx_cfg_find(&c, "com.apple.MobileSMS", 19);
  CHECK(r && r->mood == NX_MOOD_DEFAULT && nx_cfg_rule_mood(&c, r) == 3);
  r = nx_cfg_find(&c, "net.whatsapp.WhatsApp", 21);
  CHECK(r && nx_cfg_rule_mood(&c, r) == 7);
  r = nx_cfg_find(&c, "com.bad", 7);
  CHECK(r && r->mood == 0 && nx_cfg_rule_mood(&c, r) == 0);
  CHECK(nx_cfg_rule_mood(&c, NULL) == 3);
  /* defaultMood itself can't be 0xFF: it clamps to 0 like any unknown mood */
  p[2] = 0xFF;
  CHECK(nx_cfg_parse(p, base, 30, &d) == NX_OK && d.default_mood == 0);
  CHECK(nx_cfg_rule_mood(&d, nx_cfg_find(&d, "com.apple.MobileSMS", 19)) == 0);
  p[2] = 3;

  /* with ext: calls off, then on */
  p[n++] = 0x00;
  CHECK(nx_cfg_parse(p, n, 30, &c) == NX_OK && c.ext == 0 && !(c.ext & NX_EXT_CALLS));
  p[n - 1] = 0x01;
  CHECK(nx_cfg_parse(p, n, 30, &c) == NX_OK && c.ext == NX_EXT_CALLS);
  /* two trailing bytes: bad */
  p[n++] = 0;
  CHECK(nx_cfg_parse(p, n, 30, &d) == NX_BAD_MSG);
  n--;
  /* /state.bin round trip keeps 0xFF and ext; an old file (no ext) loads */
  c.ext = 0;
  wl = nx_cfg_write(&c, w);
  CHECK(wl == n && w[wl - 1] == 0);
  CHECK(nx_cfg_parse(w, wl, 30, &d) == NX_OK && memcmp(&c, &d, sizeof(c)) == 0);
  CHECK(nx_cfg_parse(w, wl - 1, 30, &d) == NX_OK && d.ext == NX_EXT_CALLS);
  /* defaults */
  nx_cfg_default(&d);
  CHECK(d.ext == NX_EXT_CALLS && d.default_mood == 5);
}

/* ---- phase 3: ANCS ----------------------------------------------------- */
static void uuid_le(const char *s, uint8_t out[16])
{
  int n = 0;
  while (*s && n < 16) {
    unsigned v;
    if (*s == '-') { s++; continue; }
    sscanf(s, "%2x", &v);
    out[15 - n++] = (uint8_t)v;
    s += 2;
  }
}

static void test_ancs_basics(void)
{
  uint8_t u[16], req[NX_ANCS_REQ_MAX], adv[31];
  nx_ancs_ns_t ev;
  const uint8_t ns[8] = { 0, 0x05, 1, 2, 0x78, 0x56, 0x34, 0x12 };
  size_t n;
  uuid_le("7905F431-B5CE-4E99-A40F-4B1E122D00D0", u);
  CHECK(memcmp(u, nx_ancs_uuid_svc, 16) == 0);
  uuid_le("9FBF120D-6301-42D9-8C58-25E699A21DBD", u);
  CHECK(memcmp(u, nx_ancs_uuid_ns, 16) == 0);
  uuid_le("69D1D8F3-45E1-49A8-9821-9BBDFDAAD9D9", u);
  CHECK(memcmp(u, nx_ancs_uuid_cp, 16) == 0);
  uuid_le("22EAC6E9-24D6-4BB5-BE44-B36ACE7C7BFB", u);
  CHECK(memcmp(u, nx_ancs_uuid_ds, 16) == 0);

  /* Notification Source */
  CHECK(nx_ancs_ns_decode(ns, 8, &ev) == NX_OK);
  CHECK(ev.event == NX_ANCS_EV_ADDED && ev.flags == (NX_ANCS_F_SILENT | NX_ANCS_F_PREEXISTING));
  CHECK(ev.category == NX_ANCS_CAT_INCOMING_CALL && ev.count == 2 && ev.uid == 0x12345678u);
  CHECK(nx_ancs_ns_decode(ns, 7, &ev) == NX_BAD_MSG);

  /* Control Point requests */
  n = nx_ancs_req_notif(0xA1B2C3D4u, req);
  {
    const uint8_t want[12] = { 0, 0xD4, 0xC3, 0xB2, 0xA1, 0, 1, 64, 0, 3, 255, 0 };
    CHECK(n == 12 && memcmp(req, want, 12) == 0);
  }
  n = nx_ancs_req_app((const uint8_t *)"com.x", 5, req);
  {
    const uint8_t want[8] = { 1, 'c', 'o', 'm', '.', 'x', 0, 0 };
    CHECK(n == 8 && memcmp(req, want, 8) == 0);
  }

  /* advertising payloads: 21 bytes, flags + one 128-bit UUID AD structure */
  n = nx_adv_flags_uuid(adv, NX_AD_SOLICIT128, nx_ancs_uuid_svc);
  CHECK(n == 21 && adv[0] == 2 && adv[1] == 1 && adv[2] == 6 && adv[3] == 17 && adv[4] == 0x15);
  CHECK(adv[5] == 0xD0 && adv[20] == 0x79);
}

/* Data Source answer for Get Notification Attributes. */
static size_t ds_notif(uint8_t *p, uint32_t uid, const char *app, const char *title, const char *msg)
{
  size_t n = 0;
  const char *v[3];
  const uint8_t ids[3] = { NX_ANCS_ATTR_APP_ID, NX_ANCS_ATTR_TITLE, NX_ANCS_ATTR_MESSAGE };
  int i;
  v[0] = app; v[1] = title; v[2] = msg;
  p[n++] = 0;
  p[n++] = (uint8_t)uid; p[n++] = (uint8_t)(uid >> 8); p[n++] = (uint8_t)(uid >> 16); p[n++] = (uint8_t)(uid >> 24);
  for (i = 0; i < 3; i++) {
    size_t l = strlen(v[i]);
    p[n++] = ids[i];
    nx_put16(p + n, (uint16_t)l); n += 2;
    memcpy(p + n, v[i], l); n += l;
  }
  return n;
}

static size_t ds_app(uint8_t *p, const char *app, const char *name)
{
  size_t n = 0, l = strlen(app);
  p[n++] = 1;
  memcpy(p + n, app, l + 1); n += l + 1;
  p[n++] = 0;
  l = strlen(name);
  nx_put16(p + n, (uint16_t)l); n += 2;
  memcpy(p + n, name, l); n += l;
  return n;
}

static void test_ancs_rsp(void)
{
  static nx_ancs_rsp_t r;
  uint8_t p[700];
  size_t n, i;
  const nx_ancs_attr_t *a;

  /* nothing expected: ignored */
  nx_ancs_rsp_clear(&r);
  n = ds_notif(p, 7, "com.x", "T", "M");
  CHECK(nx_ancs_rsp_feed(&r, p, n) == NX_ANCS_RSP_IGNORED);

  /* one notification */
  nx_ancs_rsp_expect_notif(&r, 7, 3);
  CHECK(nx_ancs_rsp_feed(&r, p, n) == NX_ANCS_RSP_DONE);
  a = nx_ancs_rsp_get(&r, NX_ANCS_ATTR_APP_ID);
  CHECK(a && a->len == 5 && memcmp(a->val, "com.x", 5) == 0);
  a = nx_ancs_rsp_get(&r, NX_ANCS_ATTR_MESSAGE);
  CHECK(a && a->len == 1 && a->val[0] == 'M');

  /* split at every possible point, then byte by byte */
  n = ds_notif(p, 0xDEADBEEFu, "net.whatsapp.WhatsApp", "Alice", "See you at eight, bring the cake");
  for (i = 1; i < n; i++) {
    nx_ancs_rsp_expect_notif(&r, 0xDEADBEEFu, 3);
    CHECK(nx_ancs_rsp_feed(&r, p, i) == NX_ANCS_RSP_MORE);
    CHECK(nx_ancs_rsp_feed(&r, p + i, n - i) == NX_ANCS_RSP_DONE);
  }
  nx_ancs_rsp_expect_notif(&r, 0xDEADBEEFu, 3);
  for (i = 0; i + 1 < n; i++) CHECK(nx_ancs_rsp_feed(&r, p + i, 1) == NX_ANCS_RSP_MORE);
  CHECK(nx_ancs_rsp_feed(&r, p + n - 1, 1) == NX_ANCS_RSP_DONE);
  a = nx_ancs_rsp_get(&r, NX_ANCS_ATTR_TITLE);
  CHECK(a && a->len == 5 && memcmp(a->val, "Alice", 5) == 0);

  /* another UID's answer (a late one) is ignored; ours still completes */
  nx_ancs_rsp_expect_notif(&r, 8, 3);
  CHECK(nx_ancs_rsp_feed(&r, p, n) == NX_ANCS_RSP_IGNORED);
  n = ds_notif(p, 8, "a", "", "");
  CHECK(nx_ancs_rsp_feed(&r, p, n) == NX_ANCS_RSP_DONE);
  a = nx_ancs_rsp_get(&r, NX_ANCS_ATTR_TITLE);
  CHECK(a && a->len == 0);
  /* a continuation that doesn't fit the header we have so far: error */
  nx_ancs_rsp_expect_notif(&r, 8, 3);
  CHECK(nx_ancs_rsp_feed(&r, p, 2) == NX_ANCS_RSP_MORE);
  {
    const uint8_t bad[3] = { 0x99, 0, 0 };
    CHECK(nx_ancs_rsp_feed(&r, bad, 3) == NX_ANCS_RSP_ERROR);
  }
  /* too large */
  nx_ancs_rsp_expect_notif(&r, 9, 3);
  memset(p, 0, sizeof(p));
  p[0] = 0; p[1] = 9; p[5] = 0; nx_put16(p + 6, 650);
  CHECK(nx_ancs_rsp_feed(&r, p, 300) == NX_ANCS_RSP_MORE);
  CHECK(nx_ancs_rsp_feed(&r, p + 300, 360) == NX_ANCS_RSP_ERROR);

  /* Get App Attributes: matched on the AppIdentifier */
  nx_ancs_rsp_expect_app(&r, (const uint8_t *)"com.x", 5, 1);
  n = ds_app(p, "com.y", "Y");
  CHECK(nx_ancs_rsp_feed(&r, p, n) == NX_ANCS_RSP_IGNORED);
  n = ds_app(p, "com.xx", "Y");
  CHECK(nx_ancs_rsp_feed(&r, p, n) == NX_ANCS_RSP_IGNORED); /* the NUL must match too */
  n = ds_app(p, "com.x", "X App");
  CHECK(nx_ancs_rsp_feed(&r, p, 4) == NX_ANCS_RSP_MORE);
  CHECK(nx_ancs_rsp_feed(&r, p + 4, n - 4) == NX_ANCS_RSP_DONE);
  a = nx_ancs_rsp_get(&r, NX_ANCS_APP_ATTR_NAME);
  CHECK(a && a->len == 5 && memcmp(a->val, "X App", 5) == 0);
}

static void test_lru(void)
{
  static nx_lru_t l;
  uint8_t out[NX_SEEN_APPS_MAX];
  char id[16];
  int i;
  size_t n;
  const nx_app_t *e;
  nx_lru_clear(&l);
  CHECK(nx_lru_encode(&l, out) == 1 && out[0] == 0);
  CHECK(nx_lru_get(&l, (const uint8_t *)"a", 1) == NULL);
  for (i = 0; i < 33; i++) {
    snprintf(id, sizeof(id), "app%02d", i);
    CHECK(nx_lru_put(&l, (const uint8_t *)id, 5, (const uint8_t *)id, 5) == 1);
  }
  CHECK(l.n == NX_LRU_N);
  CHECK(nx_lru_get(&l, (const uint8_t *)"app00", 5) == NULL); /* evicted: least recent */
  CHECK(memcmp(l.e[0].id, "app32", 5) == 0 && memcmp(l.e[31].id, "app01", 5) == 0);
  /* a hit becomes the most recent and is spared */
  e = nx_lru_get(&l, (const uint8_t *)"app01", 5);
  CHECK(e == &l.e[0] && memcmp(e->id, "app01", 5) == 0);
  CHECK(nx_lru_put(&l, (const uint8_t *)"new", 3, (const uint8_t *)"N", 1) == 1);
  CHECK(nx_lru_get(&l, (const uint8_t *)"app01", 5) != NULL);
  CHECK(nx_lru_get(&l, (const uint8_t *)"app02", 5) == NULL);
  /* update in place: no duplicate, name replaced */
  n = l.n;
  CHECK(nx_lru_put(&l, (const uint8_t *)"new", 3, (const uint8_t *)"Newer", 5) == 1);
  CHECK(l.n == n && l.e[0].name_len == 5 && memcmp(l.e[0].name, "Newer", 5) == 0);
  /* id too long: not stored; empty id: not stored */
  {
    uint8_t big[NX_APPID_MAX + 1];
    memset(big, 'x', sizeof(big));
    CHECK(nx_lru_put(&l, big, sizeof(big), (const uint8_t *)"B", 1) == 0);
    CHECK(nx_lru_put(&l, big, 0, (const uint8_t *)"B", 1) == 0);
    CHECK(nx_lru_put(&l, big, NX_APPID_MAX, (const uint8_t *)"B", 1) == 1);
  }
  /* long names are cut on a UTF-8 character boundary */
  {
    uint8_t name[40];
    memset(name, 'a', 31);
    name[31] = 0xC3; name[32] = 0xA9; /* 'e acute' straddles byte 32 */
    memset(name + 33, 'b', 7);
    CHECK(nx_lru_put(&l, (const uint8_t *)"utf", 3, name, 40) == 1);
    CHECK(l.e[0].name_len == 31);
    name[30] = 0xC3; name[31] = 0xA9; name[32] = 'b'; /* now it ends exactly at 32 */
    CHECK(nx_lru_put(&l, (const uint8_t *)"utf", 3, name, 40) == 1);
    CHECK(l.e[0].name_len == 32);
  }

  /* LIST_SEEN_APPS layout, newest first */
  nx_lru_clear(&l);
  nx_lru_put(&l, (const uint8_t *)"com.a", 5, (const uint8_t *)"A", 1);
  nx_lru_put(&l, (const uint8_t *)"com.bb", 6, (const uint8_t *)"", 0);
  n = nx_lru_encode(&l, out);
  {
    const uint8_t want[] = { 2, 6, 'c', 'o', 'm', '.', 'b', 'b', 0, 5, 'c', 'o', 'm', '.', 'a', 1, 'A' };
    CHECK(n == sizeof(want) && memcmp(out, want, n) == 0);
  }
  /* the largest reply fits maxPayload with the status byte */
  CHECK(NX_SEEN_APPS_MAX + 1 <= NX_MAX_PAYLOAD);
}

/* State machine with a fake Control Point. */
static struct {
  uint8_t req[8][NX_ANCS_REQ_MAX];
  size_t req_len[8];
  int nreq;
  int fail_writes;
  nx_ancs_note_t note[8];
  uint8_t note_app[8][64], note_name[8][64], note_title[8][64], note_msg[8][256];
  int nnote;
} F;

static int fake_cp(void *ctx, const uint8_t *d, size_t n)
{
  (void)ctx;
  if (F.fail_writes) return 1;
  if (F.nreq < 8) {
    memcpy(F.req[F.nreq], d, n);
    F.req_len[F.nreq] = n;
  }
  F.nreq++;
  return 0;
}

static void fake_note(void *ctx, const nx_ancs_note_t *n)
{
  int i = F.nnote++;
  (void)ctx;
  if (i >= 8) return;
  F.note[i] = *n;
  if (n->app_id) memcpy(F.note_app[i], n->app_id, n->app_id_len);
  if (n->app_name) memcpy(F.note_name[i], n->app_name, n->app_name_len);
  if (n->title) memcpy(F.note_title[i], n->title, n->title_len);
  if (n->msg) memcpy(F.note_msg[i], n->msg, n->msg_len);
}

static void ns_ev(nx_ancs_t *a, uint8_t event, uint8_t flags, uint8_t cat, uint32_t uid, uint32_t t)
{
  uint8_t p[8] = { event, flags, cat, 1 };
  p[4] = (uint8_t)uid; p[5] = (uint8_t)(uid >> 8); p[6] = (uint8_t)(uid >> 16); p[7] = (uint8_t)(uid >> 24);
  nx_ancs_on_ns(a, p, 8, t);
}

static void test_ancs_client(void)
{
  static nx_ancs_t a;
  static nx_lru_t lru;
  const nx_ancs_ops_t ops = { fake_cp, fake_note, NULL };
  uint8_t p[700];
  size_t n;
  uint32_t t = 1000;

  memset(&F, 0, sizeof(F));
  nx_lru_clear(&lru);
  nx_ancs_init(&a, &ops, &lru);
  /* not started: everything is ignored */
  ns_ev(&a, NX_ANCS_EV_ADDED, 0, 0, 1, t);
  CHECK(F.nreq == 0);
  nx_ancs_start(&a);

  /* PreExisting and Silent are skipped */
  ns_ev(&a, NX_ANCS_EV_ADDED, NX_ANCS_F_PREEXISTING, 4, 1, t);
  ns_ev(&a, NX_ANCS_EV_ADDED, NX_ANCS_F_SILENT, 4, 2, t);
  ns_ev(&a, NX_ANCS_EV_MODIFIED, 0, 4, 3, t);
  CHECK(F.nreq == 0 && F.nnote == 0);

  /* a message: notification attributes, then the app name (split answers) */
  ns_ev(&a, NX_ANCS_EV_ADDED, NX_ANCS_F_IMPORTANT, 4, 10, t);
  ns_ev(&a, NX_ANCS_EV_ADDED, 0, 4, 11, t); /* queued behind 10 */
  CHECK(F.nreq == 1 && F.req[0][0] == 0 && nx_get32(F.req[0] + 1) == 10);
  nx_ancs_cp_result(&a, 0, t);
  n = ds_notif(p, 10, "com.apple.MobileSMS", "Bob", "Lunch?");
  nx_ancs_on_ds(&a, p, 9, t);
  CHECK(F.nreq == 1);
  nx_ancs_on_ds(&a, p + 9, n - 9, t);
  CHECK(F.nreq == 2 && F.req[1][0] == 1 && memcmp(F.req[1] + 1, "com.apple.MobileSMS", 20) == 0);
  CHECK(F.nnote == 0);
  n = ds_app(p, "com.apple.MobileSMS", "Messages");
  nx_ancs_on_ds(&a, p, n, t); /* the answer may come before the write response */
  nx_ancs_cp_result(&a, 0, t);
  CHECK(F.nnote == 1 && F.note[0].type == NX_ANCS_NOTE_ADDED && F.note[0].uid == 10 && F.note[0].category == 4);
  CHECK(F.note[0].app_name_len == 8 && memcmp(F.note_name[0], "Messages", 8) == 0);
  CHECK(F.note[0].title_len == 3 && memcmp(F.note_title[0], "Bob", 3) == 0);
  CHECK(F.note[0].msg_len == 6 && memcmp(F.note_msg[0], "Lunch?", 6) == 0);
  CHECK(lru.n == 1);
  /* 11 went out right after; same app: cached name, no app request */
  CHECK(F.nreq == 3 && nx_get32(F.req[2] + 1) == 11);
  n = ds_notif(p, 11, "com.apple.MobileSMS", "Bob", "2");
  nx_ancs_on_ds(&a, p, n, t);
  CHECK(F.nreq == 3 && F.nnote == 2 && memcmp(F.note_name[1], "Messages", 8) == 0);
  CHECK(a.state == NX_ANCS_IDLE);

  /* Control Point error (the notification is gone): dropped, next one goes */
  ns_ev(&a, NX_ANCS_EV_ADDED, 0, 4, 20, t);
  ns_ev(&a, NX_ANCS_EV_ADDED, 0, 4, 21, t);
  CHECK(F.nreq == 4);
  nx_ancs_cp_result(&a, NX_ANCS_ERR_INVALID_PARAM, t);
  CHECK(F.nreq == 5 && nx_get32(F.req[4] + 1) == 21 && F.nnote == 2);
  /* timeout: dropped too */
  nx_ancs_tick(&a, t + NX_ANCS_REQ_MS - 1);
  CHECK(a.state == NX_ANCS_WAIT_NOTIF);
  nx_ancs_tick(&a, t + NX_ANCS_REQ_MS);
  CHECK(a.state == NX_ANCS_IDLE && F.nnote == 2 && a.n_failed == 2);
  /* its late answer is ignored */
  n = ds_notif(p, 21, "com.apple.MobileSMS", "late", "");
  nx_ancs_on_ds(&a, p, n, t);
  CHECK(F.nnote == 2);

  /* app name fails: the card still shows, named by its bundle id, not cached */
  t += 10000;
  ns_ev(&a, NX_ANCS_EV_ADDED, 0, 4, 30, t);
  n = ds_notif(p, 30, "com.z", "Z", "");
  nx_ancs_on_ds(&a, p, n, t);
  CHECK(a.state == NX_ANCS_WAIT_APP);
  nx_ancs_cp_result(&a, NX_ANCS_ERR_UNKNOWN_CMD, t);
  CHECK(F.nnote == 3 && F.note[2].app_name_len == 5 && memcmp(F.note_name[2], "com.z", 5) == 0);
  CHECK(nx_lru_get(&lru, (const uint8_t *)"com.z", 5) == NULL);

  /* incoming call, then its removal: REMOVED with the call category */
  ns_ev(&a, NX_ANCS_EV_ADDED, 0, NX_ANCS_CAT_INCOMING_CALL, 40, t);
  n = ds_notif(p, 40, "com.apple.mobilephone", "Carol", "");
  nx_ancs_on_ds(&a, p, n, t);
  n = ds_app(p, "com.apple.mobilephone", "Phone");
  nx_ancs_on_ds(&a, p, n, t);
  CHECK(F.nnote == 4 && F.note[3].category == NX_ANCS_CAT_INCOMING_CALL && memcmp(F.note_title[3], "Carol", 5) == 0);
  ns_ev(&a, NX_ANCS_EV_REMOVED, 0, NX_ANCS_CAT_INCOMING_CALL, 40, t);
  CHECK(F.nnote == 5 && F.note[4].type == NX_ANCS_NOTE_REMOVED && F.note[4].uid == 40 &&
        F.note[4].category == NX_ANCS_CAT_INCOMING_CALL);

  /* removed while its attributes are fetched: no card, the removal is reported */
  ns_ev(&a, NX_ANCS_EV_ADDED, 0, NX_ANCS_CAT_INCOMING_CALL, 50, t);
  ns_ev(&a, NX_ANCS_EV_ADDED, 0, 4, 51, t);
  ns_ev(&a, NX_ANCS_EV_REMOVED, 0, 4, 51, t); /* still queued: just dropped */
  ns_ev(&a, NX_ANCS_EV_REMOVED, 0, NX_ANCS_CAT_INCOMING_CALL, 50, t);
  CHECK(F.nnote == 7);
  n = ds_notif(p, 50, "com.apple.mobilephone", "Dave", "");
  nx_ancs_on_ds(&a, p, n, t);
  CHECK(F.nnote == 7 && a.state == NX_ANCS_IDLE && a.npend == 0);

  /* writes refused by the stack: nothing hangs */
  F.fail_writes = 1;
  ns_ev(&a, NX_ANCS_EV_ADDED, 0, 4, 60, t);
  CHECK(a.state == NX_ANCS_IDLE && a.npend == 0);
  F.fail_writes = 0;

  /* the pending list is bounded */
  F.nreq = 0;
  for (n = 0; n < NX_ANCS_PENDING + 5; n++) ns_ev(&a, NX_ANCS_EV_ADDED, 0, 4, (uint32_t)(100 + n), t);
  CHECK(a.npend == NX_ANCS_PENDING && F.nreq == 1);
  /* stop drops the work but keeps the seen apps */
  nx_ancs_stop(&a);
  CHECK(a.npend == 0 && a.state == NX_ANCS_IDLE && lru.n == 2);
  ns_ev(&a, NX_ANCS_EV_ADDED, 0, 4, 200, t);
  CHECK(F.nreq == 1);
}

int main(void)
{
  test_sha_hmac();
  test_auth();
  test_ids_qr();
  test_info();
  test_rx();
  test_cfg();
  test_notify_put();
  test_text();
  test_cfg_ext_mood();
  test_ancs_basics();
  test_ancs_rsp();
  test_lru();
  test_ancs_client();
  if (fails) {
    fprintf(stderr, "%d check(s) failed\n", fails);
    return 1;
  }
  printf("nixxa core tests: OK\n");
  return 0;
}
