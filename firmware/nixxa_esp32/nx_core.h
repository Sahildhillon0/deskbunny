/*
 * nixxa: portable protocol core (C99, no malloc, no I/O).
 * Compiled into the ESP32 sketch and into the host tests (tests/).
 * Contract: docs/ARCHITECTURE.md, "ESP32 + BLE (nixxa)".
 */
#ifndef NX_CORE_H
#define NX_CORE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- protocol constants ------------------------------------------------- */
#define NX_PROTO_VERSION 1
#define NX_SLOTS 16
#define NX_MAX_PAYLOAD 12352u               /* INFO maxPayload */
#define NX_MSG_HDR 4u                       /* op u8, seq u8, len u16 */
#define NX_MSG_MAX (NX_MAX_PAYLOAD + NX_MSG_HDR)
#define NX_RX_GAP_MS 300u                   /* a write after this gap starts a message */
#define NX_INFO_SIZE 41u
#define NX_FACE_SIZE 64u
/* PUT_SLOT payload = slot u8 + face[64] + artLen u16 + art, and the GET_SLOT
 * reply = status + face[64] + artLen u16 + art; both must fit maxPayload. */
#define NX_ART_MAX (NX_MAX_PAYLOAD - 3u - NX_FACE_SIZE) /* 12285 */

enum nx_op {
  NX_OP_AUTH = 0x01,
  NX_OP_GET_STATE = 0x02,
  NX_OP_SET_TIME = 0x03,
  NX_OP_LIST_SLOTS = 0x10,
  NX_OP_PUT_SLOT = 0x11,
  NX_OP_GET_SLOT = 0x12,
  NX_OP_DELETE_SLOT = 0x13,
  NX_OP_ACTIVATE = 0x14,
  NX_OP_SET_NOTIF_CONFIG = 0x20,
  NX_OP_NOTIFY = 0x21,
  NX_OP_DISMISS = 0x22,
  NX_OP_SET_DND = 0x23,
  NX_OP_LIST_SEEN_APPS = 0x24,
  NX_OP_SET_CLOCK = 0x25,      /* 1 byte: 0 = show the face, 1..NX_CLOCK_STYLES = that clock */
  NX_OP_UNPAIR = 0x7F
};

enum nx_status {
  NX_OK = 0,
  NX_NOT_AUTHED = 1,
  NX_BAD_MSG = 2,
  NX_BAD_SLOT = 3,
  NX_NO_SPACE = 4,
  NX_BUSY = 5,
  NX_AUTH_FAILED = 6
};

/* ---- little-endian helpers ---------------------------------------------- */
static inline uint16_t nx_get16(const uint8_t *p) { return (uint16_t)(p[0] | (p[1] << 8)); }
static inline uint32_t nx_get32(const uint8_t *p)
{
  return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}
static inline void nx_put16(uint8_t *p, uint16_t v) { p[0] = (uint8_t)v; p[1] = (uint8_t)(v >> 8); }

/* ---- message reassembly ------------------------------------------------- */
typedef struct nx_rx {
  uint8_t *buf;      /* caller-owned, cap bytes (NX_MSG_MAX on the device) */
  uint32_t cap;
  uint32_t have;     /* bytes of the current message in buf */
  uint32_t last_ms;  /* time of the previous write */
  uint8_t started;   /* last_ms is valid */
  uint8_t dropping;  /* oversize message: drop until a write starts a message */
} nx_rx_t;

enum nx_rx_result {
  NX_RX_MORE = 0,    /* nothing complete yet */
  NX_RX_MSG = 1,     /* buf[0..have) is one whole message */
  NX_RX_TOOBIG = 2,  /* header announced > cap: reply BAD_MSG to op and seq */
  NX_RX_DROPPED = 3  /* write ignored (inside an oversize message) */
};

void nx_rx_init(nx_rx_t *rx, uint8_t *buf, uint32_t cap);
/* Feed one GATT write. On NX_RX_MSG the caller handles buf and then calls
 * nx_rx_reset. On NX_RX_TOOBIG op/seq name the message to reject. Bytes past
 * the end of a message in the same write are discarded (stop-and-wait). */
int nx_rx_feed(nx_rx_t *rx, const uint8_t *data, size_t len, uint32_t now_ms,
               uint8_t *op, uint8_t *seq);
void nx_rx_reset(nx_rx_t *rx);

/* ---- notification config (op 0x20) -------------------------------------- */
#define NX_RULES_MAX 32
#define NX_APPID_MAX 64

#define NX_MOOD_DEFAULT 0xFF /* rule mood: use the config's defaultMood */
#define NX_EXT_CALLS 0x01    /* ext bit0: forward calls (ANCS) */

typedef struct nx_rule {
  uint8_t id_len;
  char app_id[NX_APPID_MAX];
  uint8_t mood;  /* < mood_count, or NX_MOOD_DEFAULT */
  uint8_t flags; /* bit0 show */
} nx_rule_t;

typedef struct nx_notif_cfg {
  uint8_t enabled;
  uint8_t card_secs;
  uint8_t default_mood;
  uint16_t quiet_start;
  uint16_t quiet_end;
  uint8_t calls_in_quiet;
  uint8_t rule_count;
  nx_rule_t rules[NX_RULES_MAX];
  uint8_t ext;   /* optional trailing byte; NX_EXT_CALLS (default 1 when absent) */
} nx_notif_cfg_t;

void nx_cfg_default(nx_notif_cfg_t *c);
/* Parse a SET_NOTIF_CONFIG payload. Returns NX_OK or NX_BAD_MSG (c untouched
 * on error). cardSecs is clamped to 2..30; a rule mood of NX_MOOD_DEFAULT is
 * kept (it means defaultMood), any other mood >= mood_count -> 0. The ext
 * byte after the rules is optional: absent = NX_EXT_CALLS. */
int nx_cfg_parse(const uint8_t *p, size_t len, uint8_t mood_count, nx_notif_cfg_t *c);
/* Serialise in the SET_NOTIF_CONFIG wire format (used for /state.bin).
 * Returns the length (<= NX_CFG_WIRE_MAX). */
#define NX_CFG_WIRE_MAX (9u + NX_RULES_MAX * (1u + NX_APPID_MAX + 2u) + 1u)
size_t nx_cfg_write(const nx_notif_cfg_t *c, uint8_t *out);
/* Rule lookup: returns the rule for app_id or NULL. */
const nx_rule_t *nx_cfg_find(const nx_notif_cfg_t *c, const char *app_id, size_t id_len);
/* The mood a rule shows: its own, or defaultMood for NX_MOOD_DEFAULT
 * (r == NULL: defaultMood). */
uint8_t nx_cfg_rule_mood(const nx_notif_cfg_t *c, const nx_rule_t *r);

/* 1 when minute-of-day m is inside [start, end) (wrapping past midnight);
 * start == end means no quiet hours. */
int nx_in_quiet(uint16_t start, uint16_t end, uint16_t m);

/* ---- NOTIFY (op 0x21) ---------------------------------------------------- */
#define NX_BODY_MAX 512

typedef struct nx_notify {
  uint8_t kind;      /* 0 message, 1 incoming call, 2 call ended */
  uint32_t id;
  uint8_t mood;      /* 0xFF = rule or default */
  const uint8_t *app_id;   uint8_t app_id_len;
  const uint8_t *app_name; uint8_t app_name_len;
  const uint8_t *title;    uint8_t title_len;
  const uint8_t *body;     uint16_t body_len; /* already cut to NX_BODY_MAX */
  const uint8_t *icon;     /* NULL, or icon_px x icon_px pixels, 1 bpp, MSB first */
  uint8_t icon_px;         /* 16 (32 bytes) or 32 (128 bytes) when icon != NULL */
} nx_notify_t;

/* Icon sizes on the NOTIFY wire: a flag byte (0 none, 1 16x16, 2 32x32). */
#define NX_ICON_SMALL_PX 16
#define NX_ICON_SMALL_BYTES 32
#define NX_ICON_BIG_PX 32
#define NX_ICON_BIG_BYTES 128

/* Pointers alias p. Returns NX_OK or NX_BAD_MSG. */
int nx_notify_parse(const uint8_t *p, size_t len, nx_notify_t *n);

/* ---- PUT_SLOT (op 0x11) -------------------------------------------------- */
typedef struct nx_put {
  uint8_t slot;
  const uint8_t *face; /* 64 bytes */
  uint16_t art_len;
  const uint8_t *art;
} nx_put_t;

/* Checks lengths, slot range (NX_BAD_SLOT) and art size (NX_NO_SPACE); the
 * face CRC is checked by the caller with db_face_decode. */
int nx_put_parse(const uint8_t *p, size_t len, nx_put_t *out);

/* ---- text ---------------------------------------------------------------- */
/* UTF-8 -> printable ASCII 0x20..0x7E: each non-ASCII code point and each
 * control character becomes '?', except '\n' (kept when keep_nl) and '\t'
 * (-> space). Returns the output length (<= out_cap - 1, NUL-terminated). */
size_t nx_ascii(const uint8_t *in, size_t len, char *out, size_t out_cap, int keep_nl);

/* Greedy word wrap of s into lines of at most cols characters. starts[i] and
 * lens[i] describe line i; returns the number of lines (<= max_lines). */
size_t nx_wrap(const char *s, size_t cols, uint16_t *starts, uint8_t *lens, size_t max_lines);

/* ---- pairing ------------------------------------------------------------- */
/* "NIXXA1:<ID>:<K as 32 uppercase hex>" (46 chars + NUL). */
#define NX_QR_TEXT_LEN 46
void nx_qr_text(const char id[7], const uint8_t k[16], char out[NX_QR_TEXT_LEN + 1]);
/* Device ID: the last 3 bytes of the BT MAC as 6 uppercase hex chars. */
void nx_device_id(const uint8_t mac[6], char out[7]);
/* tag = first 16 bytes of HMAC-SHA256(k, nonce || "NIXXA-AUTH"). */
void nx_auth_tag(const uint8_t k[16], const uint8_t nonce[16], uint8_t tag[16]);
/* Constant-time compare; 1 when equal. */
int nx_ct_equal(const uint8_t *a, const uint8_t *b, size_t n);

/* INFO characteristic value (41 bytes). */
void nx_info_build(uint8_t out[NX_INFO_SIZE], uint8_t chip, uint8_t paired, const uint8_t id3[3],
                   const uint8_t nonce[16], const char *fw_version);

#ifdef __cplusplus
}
#endif

#endif /* NX_CORE_H */
