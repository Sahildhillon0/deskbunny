/* nixxa: portable protocol core. See nx_core.h. */
#include "nx_core.h"
#include "nx_sha256.h"

#include <string.h>

/* ------------------------------------------------------------------------ */
/* Reassembly                                                               */
/* ------------------------------------------------------------------------ */
void nx_rx_init(nx_rx_t *rx, uint8_t *buf, uint32_t cap)
{
  memset(rx, 0, sizeof(*rx));
  rx->buf = buf;
  rx->cap = cap;
}

void nx_rx_reset(nx_rx_t *rx) { rx->have = 0; }

int nx_rx_feed(nx_rx_t *rx, const uint8_t *data, size_t len, uint32_t now_ms,
               uint8_t *op, uint8_t *seq)
{
  uint32_t total, take;
  int gap = !rx->started || (uint32_t)(now_ms - rx->last_ms) >= NX_RX_GAP_MS;
  rx->last_ms = now_ms;
  rx->started = 1;
  if (gap) {
    /* A write after a 300 ms pause always starts a new message: it ends an
     * oversize message being dropped and abandons a half-received one. */
    rx->dropping = 0;
    rx->have = 0;
  }
  if (rx->dropping) return NX_RX_DROPPED;

  /* Header first (it may in theory arrive split across writes). */
  while (rx->have < NX_MSG_HDR && len > 0) {
    rx->buf[rx->have++] = *data++;
    len--;
  }
  if (rx->have < NX_MSG_HDR) return NX_RX_MORE;

  total = NX_MSG_HDR + nx_get16(rx->buf + 2);
  if (total > rx->cap) {
    if (op) *op = rx->buf[0];
    if (seq) *seq = rx->buf[1];
    rx->have = 0;
    rx->dropping = 1;
    return NX_RX_TOOBIG;
  }
  take = total - rx->have;
  if (take > len) take = (uint32_t)len;
  memcpy(rx->buf + rx->have, data, take);
  rx->have += take;
  if (rx->have < total) return NX_RX_MORE;
  if (op) *op = rx->buf[0];
  if (seq) *seq = rx->buf[1];
  return NX_RX_MSG; /* trailing bytes of this write, if any, are discarded */
}

/* ------------------------------------------------------------------------ */
/* Notification config                                                      */
/* ------------------------------------------------------------------------ */
void nx_cfg_default(nx_notif_cfg_t *c)
{
  memset(c, 0, sizeof(*c));
  c->enabled = 1;
  c->card_secs = 6;
  c->default_mood = 5; /* surprised */
  c->ext = NX_EXT_CALLS;
}

static uint8_t clamp_mood(uint8_t m, uint8_t count) { return m < count ? m : 0; }

int nx_cfg_parse(const uint8_t *p, size_t len, uint8_t mood_count, nx_notif_cfg_t *c)
{
  static nx_notif_cfg_t t; /* keeps 2 KB off the caller's stack; one caller at a time */
  size_t pos = 9;
  uint8_t i;
  if (len < 9) return NX_BAD_MSG;
  memset(&t, 0, sizeof(t));
  t.enabled = p[0] ? 1 : 0;
  t.card_secs = p[1] < 2 ? 2 : (p[1] > 30 ? 30 : p[1]);
  t.default_mood = clamp_mood(p[2], mood_count);
  t.quiet_start = nx_get16(p + 3);
  t.quiet_end = nx_get16(p + 5);
  if (t.quiet_start >= 1440 || t.quiet_end >= 1440) return NX_BAD_MSG;
  t.calls_in_quiet = p[7] ? 1 : 0;
  t.rule_count = p[8];
  if (t.rule_count > NX_RULES_MAX) return NX_BAD_MSG;
  for (i = 0; i < t.rule_count; i++) {
    nx_rule_t *r = &t.rules[i];
    uint8_t n;
    if (pos >= len) return NX_BAD_MSG;
    n = p[pos++];
    if (n == 0 || n > NX_APPID_MAX || pos + n + 2 > len) return NX_BAD_MSG;
    r->id_len = n;
    memcpy(r->app_id, p + pos, n);
    pos += n;
    r->mood = p[pos] == NX_MOOD_DEFAULT ? NX_MOOD_DEFAULT : clamp_mood(p[pos], mood_count);
    pos++;
    r->flags = (uint8_t)(p[pos++] & 0x01);
  }
  /* Optional trailing ext byte (phase 3); older payloads end after the rules. */
  if (pos == len) t.ext = NX_EXT_CALLS;
  else if (pos + 1 == len) t.ext = p[pos];
  else return NX_BAD_MSG;
  memcpy(c, &t, sizeof(t));
  return NX_OK;
}

size_t nx_cfg_write(const nx_notif_cfg_t *c, uint8_t *out)
{
  size_t pos = 9;
  uint8_t i;
  out[0] = c->enabled;
  out[1] = c->card_secs;
  out[2] = c->default_mood;
  nx_put16(out + 3, c->quiet_start);
  nx_put16(out + 5, c->quiet_end);
  out[7] = c->calls_in_quiet;
  out[8] = c->rule_count;
  for (i = 0; i < c->rule_count && i < NX_RULES_MAX; i++) {
    const nx_rule_t *r = &c->rules[i];
    out[pos++] = r->id_len;
    memcpy(out + pos, r->app_id, r->id_len);
    pos += r->id_len;
    out[pos++] = r->mood;
    out[pos++] = r->flags;
  }
  out[pos++] = c->ext;
  return pos;
}

const nx_rule_t *nx_cfg_find(const nx_notif_cfg_t *c, const char *app_id, size_t id_len)
{
  uint8_t i;
  for (i = 0; i < c->rule_count; i++) {
    const nx_rule_t *r = &c->rules[i];
    if (r->id_len == id_len && memcmp(r->app_id, app_id, id_len) == 0) return r;
  }
  return NULL;
}

uint8_t nx_cfg_rule_mood(const nx_notif_cfg_t *c, const nx_rule_t *r)
{
  return r && r->mood != NX_MOOD_DEFAULT ? r->mood : c->default_mood;
}

int nx_in_quiet(uint16_t start, uint16_t end, uint16_t m)
{
  if (start == end) return 0;
  if (start < end) return m >= start && m < end;
  return m >= start || m < end; /* wraps past midnight */
}

/* ------------------------------------------------------------------------ */
/* NOTIFY / PUT_SLOT                                                        */
/* ------------------------------------------------------------------------ */
int nx_notify_parse(const uint8_t *p, size_t len, nx_notify_t *n)
{
  size_t pos = 0;
  uint16_t blen;
  memset(n, 0, sizeof(*n));
  if (len < 6) return NX_BAD_MSG;
  n->kind = p[0];
  if (n->kind > 2) return NX_BAD_MSG;
  n->id = nx_get32(p + 1);
  n->mood = p[5];
  pos = 6;
#define NX_STR8(ptr, l)                       \
  do {                                        \
    if (pos >= len) return NX_BAD_MSG;        \
    l = p[pos++];                             \
    if (pos + l > len) return NX_BAD_MSG;     \
    ptr = p + pos;                            \
    pos += l;                                 \
  } while (0)
  NX_STR8(n->app_id, n->app_id_len);
  NX_STR8(n->app_name, n->app_name_len);
  NX_STR8(n->title, n->title_len);
#undef NX_STR8
  if (pos + 2 > len) return NX_BAD_MSG;
  blen = nx_get16(p + pos);
  pos += 2;
  if (pos + blen > len) return NX_BAD_MSG;
  n->body = p + pos;
  n->body_len = blen > NX_BODY_MAX ? NX_BODY_MAX : blen;
  pos += blen;
  if (pos >= len) return NX_BAD_MSG;
  if (p[pos] > 2) return NX_BAD_MSG;
  {
    /* icon flag: 0 none, 1 a 16x16 bitmap (32 bytes), 2 a 32x32 one (128 bytes) */
    uint8_t flag = p[pos++];
    if (flag) {
      uint16_t bytes = flag == 2 ? NX_ICON_BIG_BYTES : NX_ICON_SMALL_BYTES;
      if (pos + bytes > len) return NX_BAD_MSG;
      n->icon = p + pos;
      n->icon_px = flag == 2 ? NX_ICON_BIG_PX : NX_ICON_SMALL_PX;
      pos += bytes;
    }
  }
  return pos == len ? NX_OK : NX_BAD_MSG;
}

int nx_put_parse(const uint8_t *p, size_t len, nx_put_t *out)
{
  memset(out, 0, sizeof(*out));
  if (len < 1u + NX_FACE_SIZE + 2u) return NX_BAD_MSG;
  out->slot = p[0];
  out->face = p + 1;
  out->art_len = nx_get16(p + 1 + NX_FACE_SIZE);
  out->art = out->art_len ? p + 3 + NX_FACE_SIZE : NULL;
  if (len != 3u + NX_FACE_SIZE + out->art_len) return NX_BAD_MSG;
  if (out->slot >= NX_SLOTS) return NX_BAD_SLOT;
  if (out->art_len > NX_ART_MAX) return NX_NO_SPACE;
  if (out->art_len) {
    /* Drawing blob v2 header sanity; the stream itself is checked when drawn. */
    static const uint8_t magic[8] = { 'D', 'B', 'A', 'R', 'T', '0', '0', '1' };
    uint16_t frames;
    if (out->art_len < 16 || memcmp(out->art, magic, 8) != 0 || out->art[8] != 2 || out->art[9] > 2)
      return NX_BAD_MSG;
    frames = nx_get16(out->art + 10);
    if (frames > 10) return NX_BAD_MSG;
  }
  return NX_OK;
}

/* ------------------------------------------------------------------------ */
/* Text                                                                     */
/* ------------------------------------------------------------------------ */
size_t nx_ascii(const uint8_t *in, size_t len, char *out, size_t out_cap, int keep_nl)
{
  size_t o = 0, i;
  uint8_t cont = 0; /* continuation bytes still expected */
  if (out_cap == 0) return 0;
  for (i = 0; i < len && o + 1 < out_cap; i++) {
    uint8_t b = in[i];
    char c;
    if (cont && (b & 0xC0) == 0x80) { cont--; continue; }
    cont = 0;
    if (b == '\r') continue;
    if (b == '\n') c = keep_nl ? '\n' : ' ';
    else if (b == '\t') c = ' ';
    else if (b >= 0x20 && b <= 0x7E) c = (char)b;
    else {
      c = '?';
      if (b >= 0xC0 && b < 0xE0) cont = 1;
      else if (b >= 0xE0 && b < 0xF0) cont = 2;
      else if (b >= 0xF0 && b < 0xF8) cont = 3;
    }
    out[o++] = c;
  }
  out[o] = '\0';
  return o;
}

size_t nx_wrap(const char *s, size_t cols, uint16_t *starts, uint8_t *lens, size_t max_lines)
{
  size_t pos = 0, n = 0;
  if (cols == 0) return 0;
  while (s[pos] && n < max_lines) {
    size_t i, brk, next;
    while (s[pos] == ' ') pos++; /* no leading blanks on a line */
    if (!s[pos]) break;
    /* the line holds at most cols chars, or stops at a newline */
    for (i = 0; i < cols && s[pos + i] && s[pos + i] != '\n'; i++) {}
    if (s[pos + i] == '\n' || s[pos + i] == '\0') {
      brk = i;
      next = pos + i + (s[pos + i] == '\n');
    } else if (s[pos + i] == ' ') {
      brk = i; /* the word ends exactly at the edge */
      next = pos + i + 1;
    } else {
      size_t j = i;
      while (j > 0 && s[pos + j - 1] != ' ') j--;
      if (j > 0) { brk = j - 1; next = pos + j; }
      else { brk = i; next = pos + i; } /* one word longer than the line */
    }
    while (brk > 0 && s[pos + brk - 1] == ' ') brk--;
    starts[n] = (uint16_t)pos;
    lens[n] = (uint8_t)brk;
    n++;
    pos = next;
  }
  return n;
}

/* ------------------------------------------------------------------------ */
/* Pairing                                                                  */
/* ------------------------------------------------------------------------ */
static const char HEXU[] = "0123456789ABCDEF";

void nx_device_id(const uint8_t mac[6], char out[7])
{
  int i;
  for (i = 0; i < 3; i++) {
    out[2 * i] = HEXU[mac[3 + i] >> 4];
    out[2 * i + 1] = HEXU[mac[3 + i] & 15];
  }
  out[6] = '\0';
}

void nx_qr_text(const char id[7], const uint8_t k[16], char out[NX_QR_TEXT_LEN + 1])
{
  int i;
  memcpy(out, "NIXXA1:", 7);
  memcpy(out + 7, id, 6);
  out[13] = ':';
  for (i = 0; i < 16; i++) {
    out[14 + 2 * i] = HEXU[k[i] >> 4];
    out[15 + 2 * i] = HEXU[k[i] & 15];
  }
  out[NX_QR_TEXT_LEN] = '\0';
}

void nx_auth_tag(const uint8_t k[16], const uint8_t nonce[16], uint8_t tag[16])
{
  uint8_t msg[16 + 10], mac[32];
  memcpy(msg, nonce, 16);
  memcpy(msg + 16, "NIXXA-AUTH", 10);
  nx_hmac_sha256(k, 16, msg, sizeof(msg), mac);
  memcpy(tag, mac, 16);
  memset(mac, 0, sizeof(mac));
}

int nx_ct_equal(const uint8_t *a, const uint8_t *b, size_t n)
{
  uint8_t d = 0;
  size_t i;
  for (i = 0; i < n; i++) d |= (uint8_t)(a[i] ^ b[i]);
  return d == 0;
}

void nx_info_build(uint8_t out[NX_INFO_SIZE], uint8_t chip, uint8_t paired, const uint8_t id3[3],
                   const uint8_t nonce[16], const char *fw_version)
{
  size_t i;
  memset(out, 0, NX_INFO_SIZE);
  out[0] = NX_PROTO_VERSION;
  out[1] = chip;
  out[2] = paired;
  out[3] = NX_SLOTS;
  nx_put16(out + 4, (uint16_t)NX_MAX_PAYLOAD);
  memcpy(out + 6, id3, 3);
  memcpy(out + 9, nonce, 16);
  for (i = 0; i < 16 && fw_version[i]; i++) out[25 + i] = (uint8_t)fw_version[i];
}
