/*
 * nixxa: ANCS (Apple Notification Center Service) client logic, portable C99
 * (no malloc, no I/O). The BLE glue (nx_ble.cpp) discovers ANCS, subscribes
 * and hands the raw Notification Source / Data Source values and the Control
 * Point write results to nx_ancs_*; this module decides what to request, keeps
 * one request in flight at a time, reassembles the Data Source responses and
 * reports finished notifications through a callback.
 * Compiled into the ESP32 sketch and into the host tests (tests/).
 * Contract: docs/ARCHITECTURE.md, "iOS (phase 3)" -> "Firmware: ANCS client".
 * ANCS itself: Apple, "Apple Notification Center Service (ANCS) Specification".
 */
#ifndef NX_ANCS_H
#define NX_ANCS_H

#include <stddef.h>
#include <stdint.h>

#include "nx_core.h"

#ifdef __cplusplus
extern "C" {
#endif

/* UUIDs, little-endian (as on air / as the stacks want them). */
extern const uint8_t nx_ancs_uuid_svc[16]; /* 7905F431-B5CE-4E99-A40F-4B1E122D00D0 */
extern const uint8_t nx_ancs_uuid_ns[16];  /* 9FBF120D-6301-42D9-8C58-25E699A21DBD Notification Source */
extern const uint8_t nx_ancs_uuid_cp[16];  /* 69D1D8F3-45E1-49A8-9821-9BBDFDAAD9D9 Control Point */
extern const uint8_t nx_ancs_uuid_ds[16];  /* 22EAC6E9-24D6-4BB5-BE44-B36ACE7C7BFB Data Source */

enum { NX_ANCS_EV_ADDED = 0, NX_ANCS_EV_MODIFIED = 1, NX_ANCS_EV_REMOVED = 2 };
enum {
  NX_ANCS_F_SILENT = 0x01,
  NX_ANCS_F_IMPORTANT = 0x02,
  NX_ANCS_F_PREEXISTING = 0x04,
  NX_ANCS_F_POSITIVE = 0x08,
  NX_ANCS_F_NEGATIVE = 0x10
};
enum { NX_ANCS_CAT_OTHER = 0, NX_ANCS_CAT_INCOMING_CALL = 1, NX_ANCS_CAT_MISSED_CALL = 2 };
enum { NX_ANCS_CMD_NOTIF_ATTRS = 0, NX_ANCS_CMD_APP_ATTRS = 1 };
enum { NX_ANCS_ATTR_APP_ID = 0, NX_ANCS_ATTR_TITLE = 1, NX_ANCS_ATTR_MESSAGE = 3 };
enum { NX_ANCS_APP_ATTR_NAME = 0 };
/* Control Point ATT error codes. */
enum {
  NX_ANCS_ERR_UNKNOWN_CMD = 0xA0,
  NX_ANCS_ERR_INVALID_CMD = 0xA1,
  NX_ANCS_ERR_INVALID_PARAM = 0xA2,
  NX_ANCS_ERR_ACTION_FAILED = 0xA3
};

#define NX_ANCS_TITLE_MAX 64   /* requested max lengths */
#define NX_ANCS_MSG_MAX 255
#define NX_ANCS_REQ_MS 4000u   /* per Control Point request (write + Data Source answer) */
#define NX_ANCS_PENDING 16     /* Added notifications waiting for their attributes */

/* ---- Notification Source ------------------------------------------------ */
typedef struct nx_ancs_ns {
  uint8_t event, flags, category, count;
  uint32_t uid;
} nx_ancs_ns_t;

/* 8 bytes: EventID, EventFlags, CategoryID, CategoryCount, NotificationUID
 * u32 LE. Longer values are accepted (future fields). NX_OK / NX_BAD_MSG. */
int nx_ancs_ns_decode(const uint8_t *p, size_t len, nx_ancs_ns_t *ev);

/* ---- Control Point requests --------------------------------------------- */
#define NX_ANCS_REQ_MAX (1u + NX_APPID_MAX + 2u)
/* Get Notification Attributes: AppIdentifier, Title(64), Message(255). 12 B. */
size_t nx_ancs_req_notif(uint32_t uid, uint8_t out[NX_ANCS_REQ_MAX]);
/* Get App Attributes: DisplayName. app_id_len <= NX_APPID_MAX. */
size_t nx_ancs_req_app(const uint8_t *app_id, uint8_t app_id_len, uint8_t out[NX_ANCS_REQ_MAX]);

/* ---- Data Source reassembly --------------------------------------------- */
/* Largest notification-attributes answer: 5 + (3 + appId) + (3 + 64) + (3 + 255),
 * with room for an AppIdentifier of up to ~250 bytes. */
#define NX_ANCS_RSP_MAX 600u
#define NX_ANCS_ATTRS_MAX 3

typedef struct nx_ancs_attr {
  uint8_t id;
  uint16_t len;
  const uint8_t *val; /* points into the reassembly buffer */
} nx_ancs_attr_t;

typedef struct nx_ancs_rsp {
  uint8_t active;
  uint8_t cmd;
  uint32_t uid;                   /* NX_ANCS_CMD_NOTIF_ATTRS */
  uint8_t app_id[NX_APPID_MAX];   /* NX_ANCS_CMD_APP_ATTRS */
  uint8_t app_id_len;
  uint8_t want;                   /* attributes requested */
  uint8_t nattr;                  /* attributes parsed so far */
  nx_ancs_attr_t attr[NX_ANCS_ATTRS_MAX];
  uint16_t have;
  uint8_t buf[NX_ANCS_RSP_MAX];
} nx_ancs_rsp_t;

enum {
  NX_ANCS_RSP_MORE = 0,   /* need more Data Source notifications */
  NX_ANCS_RSP_DONE = 1,   /* every requested attribute is in attr[] */
  NX_ANCS_RSP_IGNORED = 2,/* not ours (no request, or another command/UID/app) */
  NX_ANCS_RSP_ERROR = 3   /* malformed or too large: request failed */
};

void nx_ancs_rsp_expect_notif(nx_ancs_rsp_t *r, uint32_t uid, uint8_t want);
void nx_ancs_rsp_expect_app(nx_ancs_rsp_t *r, const uint8_t *app_id, uint8_t len, uint8_t want);
void nx_ancs_rsp_clear(nx_ancs_rsp_t *r);
/* Feed one Data Source notification. A response is matched by its header
 * (CommandID + NotificationUID, or CommandID + AppIdentifier), so a stray
 * fragment that arrives with no partial response pending is ignored. */
int nx_ancs_rsp_feed(nx_ancs_rsp_t *r, const uint8_t *p, size_t len);
const nx_ancs_attr_t *nx_ancs_rsp_get(const nx_ancs_rsp_t *r, uint8_t id);

/* ---- Seen apps: 32-entry LRU of AppIdentifier -> DisplayName -------------- */
#define NX_LRU_N 32
#define NX_APPNAME_MAX 32
typedef struct nx_app {
  uint8_t id_len, name_len;
  uint8_t id[NX_APPID_MAX];
  uint8_t name[NX_APPNAME_MAX];
} nx_app_t;
typedef struct nx_lru {
  uint8_t n;
  nx_app_t e[NX_LRU_N]; /* e[0] = most recently used */
} nx_lru_t;

void nx_lru_clear(nx_lru_t *l);
/* Lookup; a hit becomes the most recent entry (returned, == &l->e[0]). */
const nx_app_t *nx_lru_get(nx_lru_t *l, const uint8_t *id, size_t id_len);
/* Insert or update as the most recent entry; evicts the least recent one.
 * Ids longer than NX_APPID_MAX are not stored (0 returned); names are cut to
 * NX_APPNAME_MAX bytes on a UTF-8 character boundary. */
int nx_lru_put(nx_lru_t *l, const uint8_t *id, size_t id_len, const uint8_t *name, size_t name_len);
/* LIST_SEEN_APPS (op 0x24) reply payload after the status byte:
 * count u8, then per app (newest first) idLen u8, appId, nameLen u8, name. */
#define NX_SEEN_APPS_MAX (1u + NX_LRU_N * (2u + NX_APPID_MAX + NX_APPNAME_MAX))
size_t nx_lru_encode(const nx_lru_t *l, uint8_t out[NX_SEEN_APPS_MAX]);

/* ---- client state machine ------------------------------------------------ */
enum { NX_ANCS_NOTE_ADDED = 0, NX_ANCS_NOTE_REMOVED = 1 };

/* A finished notification (ADDED, after its attributes and app name were
 * fetched) or a removal (REMOVED: only uid and category are set). Pointers are
 * valid during the callback only. */
typedef struct nx_ancs_note {
  uint8_t type;
  uint8_t category;
  uint32_t uid;
  const uint8_t *app_id;   uint8_t app_id_len;
  const uint8_t *app_name; uint8_t app_name_len;
  const uint8_t *title;    uint8_t title_len;
  const uint8_t *msg;      uint16_t msg_len;
} nx_ancs_note_t;

typedef struct nx_ancs_ops {
  /* Write to the Control Point (write with response). Return 0 when the write
   * was handed to the stack; its result must then come back through
   * nx_ancs_cp_result. Nonzero fails the request at once. */
  int (*cp_write)(void *ctx, const uint8_t *d, size_t n);
  void (*note)(void *ctx, const nx_ancs_note_t *n);
  void *ctx;
} nx_ancs_ops_t;

enum { NX_ANCS_IDLE = 0, NX_ANCS_WAIT_NOTIF = 1, NX_ANCS_WAIT_APP = 2 };

typedef struct nx_ancs {
  nx_ancs_ops_t ops;
  nx_lru_t *lru;
  uint8_t on;
  struct { uint32_t uid; uint8_t cat; } pend[NX_ANCS_PENDING];
  uint8_t npend;
  /* request in flight */
  uint8_t state;
  uint8_t cur_removed;   /* Removed arrived while its attributes were fetched */
  uint32_t t0;
  uint32_t cur_uid;
  uint8_t cur_cat;
  /* notification attributes, held while the app name is fetched */
  uint8_t app_id[NX_APPID_MAX]; uint8_t app_id_len;
  uint8_t title[NX_ANCS_TITLE_MAX]; uint8_t title_len;
  uint8_t msg[NX_ANCS_MSG_MAX]; uint16_t msg_len;
  nx_ancs_rsp_t rsp;
  /* counters, for the serial 'S' command */
  uint16_t n_shown, n_failed;
} nx_ancs_t;

/* lru outlives connections (it backs LIST_SEEN_APPS). */
void nx_ancs_init(nx_ancs_t *a, const nx_ancs_ops_t *ops, nx_lru_t *lru);
/* ANCS found and Data Source subscribed: start taking events. */
void nx_ancs_start(nx_ancs_t *a);
/* Link gone: drop the pending work (the LRU is kept). */
void nx_ancs_stop(nx_ancs_t *a);
void nx_ancs_on_ns(nx_ancs_t *a, const uint8_t *p, size_t len, uint32_t now_ms);
void nx_ancs_on_ds(nx_ancs_t *a, const uint8_t *p, size_t len, uint32_t now_ms);
/* Result of the last cp_write: 0 = success, else the ATT error code
 * (0xA0..0xA3 from ANCS, anything else from the stack). */
void nx_ancs_cp_result(nx_ancs_t *a, uint8_t att_status, uint32_t now_ms);
/* Call regularly (every 100 ms is plenty): request timeouts. */
void nx_ancs_tick(nx_ancs_t *a, uint32_t now_ms);

/* ---- advertising ---------------------------------------------------------- */
/* Flags (LE General Discoverable, BR/EDR not supported) + one 128-bit UUID in
 * an AD structure of type ad_type (0x07 complete list of service UUIDs, 0x15
 * service solicitation). Returns 21. */
#define NX_AD_SVC128 0x07
#define NX_AD_SOLICIT128 0x15
size_t nx_adv_flags_uuid(uint8_t out[31], uint8_t ad_type, const uint8_t uuid_le[16]);

#ifdef __cplusplus
}
#endif

#endif /* NX_ANCS_H */
