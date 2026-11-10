/* nixxa: ANCS client logic. See nx_ancs.h. */
#include "nx_ancs.h"

#include <string.h>

const uint8_t nx_ancs_uuid_svc[16] = { 0xD0, 0x00, 0x2D, 0x12, 0x1E, 0x4B, 0x0F, 0xA4,
                                       0x99, 0x4E, 0xCE, 0xB5, 0x31, 0xF4, 0x05, 0x79 };
const uint8_t nx_ancs_uuid_ns[16] = { 0xBD, 0x1D, 0xA2, 0x99, 0xE6, 0x25, 0x58, 0x8C,
                                      0xD9, 0x42, 0x01, 0x63, 0x0D, 0x12, 0xBF, 0x9F };
const uint8_t nx_ancs_uuid_cp[16] = { 0xD9, 0xD9, 0xAA, 0xFD, 0xBD, 0x9B, 0x21, 0x98,
                                      0xA8, 0x49, 0xE1, 0x45, 0xF3, 0xD8, 0xD1, 0x69 };
const uint8_t nx_ancs_uuid_ds[16] = { 0xFB, 0x7B, 0x7C, 0xCE, 0x6A, 0xB3, 0x44, 0xBE,
                                      0xB5, 0x4B, 0xD6, 0x24, 0xE9, 0xC6, 0xEA, 0x22 };

/* ------------------------------------------------------------------------ */
/* Notification Source / Control Point                                      */
/* ------------------------------------------------------------------------ */
int nx_ancs_ns_decode(const uint8_t *p, size_t len, nx_ancs_ns_t *ev)
{
  if (len < 8) return NX_BAD_MSG;
  ev->event = p[0];
  ev->flags = p[1];
  ev->category = p[2];
  ev->count = p[3];
  ev->uid = nx_get32(p + 4);
  return NX_OK;
}

size_t nx_ancs_req_notif(uint32_t uid, uint8_t out[NX_ANCS_REQ_MAX])
{
  out[0] = NX_ANCS_CMD_NOTIF_ATTRS;
  out[1] = (uint8_t)uid;
  out[2] = (uint8_t)(uid >> 8);
  out[3] = (uint8_t)(uid >> 16);
  out[4] = (uint8_t)(uid >> 24);
  out[5] = NX_ANCS_ATTR_APP_ID;
  out[6] = NX_ANCS_ATTR_TITLE;
  nx_put16(out + 7, NX_ANCS_TITLE_MAX);
  out[9] = NX_ANCS_ATTR_MESSAGE;
  nx_put16(out + 10, NX_ANCS_MSG_MAX);
  return 12;
}

size_t nx_ancs_req_app(const uint8_t *app_id, uint8_t app_id_len, uint8_t out[NX_ANCS_REQ_MAX])
{
  if (app_id_len > NX_APPID_MAX) app_id_len = NX_APPID_MAX;
  out[0] = NX_ANCS_CMD_APP_ATTRS;
  memcpy(out + 1, app_id, app_id_len);
  out[1 + app_id_len] = 0; /* AppIdentifier is NUL-terminated */
  out[2 + app_id_len] = NX_ANCS_APP_ATTR_NAME;
  return 3u + app_id_len;
}

/* ------------------------------------------------------------------------ */
/* Data Source reassembly                                                   */
/* ------------------------------------------------------------------------ */
void nx_ancs_rsp_clear(nx_ancs_rsp_t *r)
{
  r->active = 0;
  r->have = 0;
  r->nattr = 0;
}

void nx_ancs_rsp_expect_notif(nx_ancs_rsp_t *r, uint32_t uid, uint8_t want)
{
  nx_ancs_rsp_clear(r);
  r->active = 1;
  r->cmd = NX_ANCS_CMD_NOTIF_ATTRS;
  r->uid = uid;
  r->app_id_len = 0;
  r->want = want;
}

void nx_ancs_rsp_expect_app(nx_ancs_rsp_t *r, const uint8_t *app_id, uint8_t len, uint8_t want)
{
  nx_ancs_rsp_clear(r);
  if (len > NX_APPID_MAX) len = NX_APPID_MAX;
  r->active = 1;
  r->cmd = NX_ANCS_CMD_APP_ATTRS;
  r->uid = 0;
  memcpy(r->app_id, app_id, len);
  r->app_id_len = len;
  r->want = want;
}

/* Expected header byte i: CommandID, then the UID (LE) or the AppIdentifier
 * and its NUL. */
static uint8_t hdr_len(const nx_ancs_rsp_t *r)
{
  return (uint8_t)(r->cmd == NX_ANCS_CMD_NOTIF_ATTRS ? 5u : 2u + r->app_id_len);
}

static uint8_t hdr_byte(const nx_ancs_rsp_t *r, size_t i)
{
  if (i == 0) return r->cmd;
  if (r->cmd == NX_ANCS_CMD_NOTIF_ATTRS) return (uint8_t)(r->uid >> (8 * (i - 1)));
  return i - 1 < r->app_id_len ? r->app_id[i - 1] : 0;
}

/* Compares the header bytes in p[0..n) that fall at offset `at` of the response. */
static int hdr_match(const nx_ancs_rsp_t *r, size_t at, const uint8_t *p, size_t n)
{
  size_t h = hdr_len(r), i;
  for (i = 0; at + i < h && i < n; i++)
    if (p[i] != hdr_byte(r, at + i)) return 0;
  return 1;
}

int nx_ancs_rsp_feed(nx_ancs_rsp_t *r, const uint8_t *p, size_t len)
{
  size_t pos;
  if (!r->active || len == 0) return NX_ANCS_RSP_IGNORED;
  if (!hdr_match(r, r->have, p, len)) {
    if (r->have == 0) return NX_ANCS_RSP_IGNORED; /* not the start of our answer */
    nx_ancs_rsp_clear(r);
    return NX_ANCS_RSP_ERROR;
  }
  if ((size_t)r->have + len > NX_ANCS_RSP_MAX) {
    nx_ancs_rsp_clear(r);
    return NX_ANCS_RSP_ERROR;
  }
  memcpy(r->buf + r->have, p, len);
  r->have = (uint16_t)(r->have + len);

  /* Parse from the top each time: answers are a few hundred bytes at most. */
  pos = hdr_len(r);
  if (r->have < pos) return NX_ANCS_RSP_MORE;
  r->nattr = 0;
  while (r->nattr < r->want) {
    uint16_t l;
    if (pos + 3 > r->have) return NX_ANCS_RSP_MORE;
    l = nx_get16(r->buf + pos + 1);
    if (pos + 3 + l > r->have) return NX_ANCS_RSP_MORE;
    if (r->nattr < NX_ANCS_ATTRS_MAX) {
      nx_ancs_attr_t *a = &r->attr[r->nattr];
      a->id = r->buf[pos];
      a->len = l;
      a->val = r->buf + pos + 3;
    }
    r->nattr++;
    pos += 3u + l;
  }
  return NX_ANCS_RSP_DONE;
}

const nx_ancs_attr_t *nx_ancs_rsp_get(const nx_ancs_rsp_t *r, uint8_t id)
{
  uint8_t i, n = r->nattr < NX_ANCS_ATTRS_MAX ? r->nattr : NX_ANCS_ATTRS_MAX;
  for (i = 0; i < n; i++)
    if (r->attr[i].id == id) return &r->attr[i];
  return NULL;
}

/* ------------------------------------------------------------------------ */
/* Seen apps (LRU)                                                          */
/* ------------------------------------------------------------------------ */
void nx_lru_clear(nx_lru_t *l) { l->n = 0; }

static void lru_to_front(nx_lru_t *l, uint8_t i)
{
  nx_app_t t;
  if (i == 0) return;
  t = l->e[i];
  memmove(&l->e[1], &l->e[0], (size_t)i * sizeof(nx_app_t));
  l->e[0] = t;
}

static int lru_find(const nx_lru_t *l, const uint8_t *id, size_t id_len)
{
  uint8_t i;
  for (i = 0; i < l->n; i++)
    if (l->e[i].id_len == id_len && memcmp(l->e[i].id, id, id_len) == 0) return i;
  return -1;
}

const nx_app_t *nx_lru_get(nx_lru_t *l, const uint8_t *id, size_t id_len)
{
  int i = lru_find(l, id, id_len);
  if (i < 0) return NULL;
  lru_to_front(l, (uint8_t)i);
  return &l->e[0];
}

int nx_lru_put(nx_lru_t *l, const uint8_t *id, size_t id_len, const uint8_t *name, size_t name_len)
{
  int i;
  nx_app_t *e;
  if (id_len == 0 || id_len > NX_APPID_MAX) return 0;
  if (name_len > NX_APPNAME_MAX) {
    name_len = NX_APPNAME_MAX;
    while (name_len > 0 && (name[name_len] & 0xC0) == 0x80) name_len--; /* don't split a character */
  }
  i = lru_find(l, id, id_len);
  if (i < 0) {
    if (l->n < NX_LRU_N) l->n++;
    i = l->n - 1; /* a new entry or, when full, the least recent one */
  }
  lru_to_front(l, (uint8_t)i);
  e = &l->e[0];
  e->id_len = (uint8_t)id_len;
  memcpy(e->id, id, id_len);
  e->name_len = (uint8_t)name_len;
  memcpy(e->name, name, name_len);
  return 1;
}

size_t nx_lru_encode(const nx_lru_t *l, uint8_t out[NX_SEEN_APPS_MAX])
{
  size_t pos = 1;
  uint8_t i;
  out[0] = l->n;
  for (i = 0; i < l->n; i++) {
    const nx_app_t *e = &l->e[i];
    out[pos++] = e->id_len;
    memcpy(out + pos, e->id, e->id_len);
    pos += e->id_len;
    out[pos++] = e->name_len;
    memcpy(out + pos, e->name, e->name_len);
    pos += e->name_len;
  }
  return pos;
}

/* ------------------------------------------------------------------------ */
/* Client state machine                                                     */
/* ------------------------------------------------------------------------ */
void nx_ancs_init(nx_ancs_t *a, const nx_ancs_ops_t *ops, nx_lru_t *lru)
{
  memset(a, 0, sizeof(*a));
  a->ops = *ops;
  a->lru = lru;
}

void nx_ancs_start(nx_ancs_t *a)
{
  nx_ancs_stop(a);
  a->on = 1;
}

void nx_ancs_stop(nx_ancs_t *a)
{
  a->on = 0;
  a->npend = 0;
  a->state = NX_ANCS_IDLE;
  nx_ancs_rsp_clear(&a->rsp);
}

static void emit_added(nx_ancs_t *a, const uint8_t *name, uint8_t name_len)
{
  nx_ancs_note_t n;
  memset(&n, 0, sizeof(n));
  n.type = NX_ANCS_NOTE_ADDED;
  n.category = a->cur_cat;
  n.uid = a->cur_uid;
  n.app_id = a->app_id;
  n.app_id_len = a->app_id_len;
  if (name_len) {
    n.app_name = name;
    n.app_name_len = name_len;
  } else { /* no display name: show the bundle id */
    n.app_name = a->app_id;
    n.app_name_len = a->app_id_len;
  }
  n.title = a->title;
  n.title_len = a->title_len;
  n.msg = a->msg;
  n.msg_len = a->msg_len;
  a->n_shown++;
  if (a->ops.note) a->ops.note(a->ops.ctx, &n);
}

static void issue(nx_ancs_t *a, uint32_t now);

/* The request in flight failed (Control Point error, timeout, bad answer). */
static void fail(nx_ancs_t *a)
{
  uint8_t st = a->state;
  a->state = NX_ANCS_IDLE;
  nx_ancs_rsp_clear(&a->rsp);
  if (st == NX_ANCS_WAIT_APP && !a->cur_removed) {
    emit_added(a, NULL, 0); /* we have everything but the app's name */
    return;
  }
  a->n_failed++;
}

static void start_app_request(nx_ancs_t *a, uint32_t now)
{
  uint8_t req[NX_ANCS_REQ_MAX];
  size_t n = nx_ancs_req_app(a->app_id, a->app_id_len, req);
  nx_ancs_rsp_expect_app(&a->rsp, a->app_id, a->app_id_len, 1);
  a->state = NX_ANCS_WAIT_APP;
  a->t0 = now;
  if (a->ops.cp_write(a->ops.ctx, req, n) != 0) fail(a);
}

static void notif_done(nx_ancs_t *a, uint32_t now)
{
  const nx_ancs_attr_t *id = nx_ancs_rsp_get(&a->rsp, NX_ANCS_ATTR_APP_ID);
  const nx_ancs_attr_t *t = nx_ancs_rsp_get(&a->rsp, NX_ANCS_ATTR_TITLE);
  const nx_ancs_attr_t *m = nx_ancs_rsp_get(&a->rsp, NX_ANCS_ATTR_MESSAGE);
  const nx_app_t *app;
  if (!id || id->len == 0 || id->len > NX_APPID_MAX || a->cur_removed) {
    /* No usable AppIdentifier (no rule can name one longer than 64 bytes). */
    a->state = NX_ANCS_WAIT_NOTIF;
    fail(a);
    return;
  }
  a->app_id_len = (uint8_t)id->len;
  memcpy(a->app_id, id->val, id->len);
  a->title_len = 0;
  if (t) {
    a->title_len = (uint8_t)(t->len > NX_ANCS_TITLE_MAX ? NX_ANCS_TITLE_MAX : t->len);
    memcpy(a->title, t->val, a->title_len);
  }
  a->msg_len = 0;
  if (m) {
    a->msg_len = (uint16_t)(m->len > NX_ANCS_MSG_MAX ? NX_ANCS_MSG_MAX : m->len);
    memcpy(a->msg, m->val, a->msg_len);
  }
  nx_ancs_rsp_clear(&a->rsp);
  app = a->lru ? nx_lru_get(a->lru, a->app_id, a->app_id_len) : NULL;
  if (app) {
    a->state = NX_ANCS_IDLE;
    emit_added(a, app->name, app->name_len);
    return;
  }
  start_app_request(a, now);
}

static void app_done(nx_ancs_t *a)
{
  const nx_ancs_attr_t *nm = nx_ancs_rsp_get(&a->rsp, NX_ANCS_APP_ATTR_NAME);
  const uint8_t *name = nm ? nm->val : NULL;
  uint8_t name_len = 0;
  a->state = NX_ANCS_IDLE;
  if (a->lru && nx_lru_put(a->lru, a->app_id, a->app_id_len, name ? name : (const uint8_t *)"",
                           nm ? nm->len : 0)) {
    name = a->lru->e[0].name;
    name_len = a->lru->e[0].name_len;
  } else if (nm) {
    name_len = (uint8_t)(nm->len > NX_APPNAME_MAX ? NX_APPNAME_MAX : nm->len);
  }
  nx_ancs_rsp_clear(&a->rsp);
  if (a->cur_removed) return;
  emit_added(a, name, name_len);
}

static void issue(nx_ancs_t *a, uint32_t now)
{
  while (a->on && a->state == NX_ANCS_IDLE && a->npend > 0) {
    uint8_t req[NX_ANCS_REQ_MAX];
    size_t n;
    a->cur_uid = a->pend[0].uid;
    a->cur_cat = a->pend[0].cat;
    a->cur_removed = 0;
    a->npend--;
    memmove(&a->pend[0], &a->pend[1], (size_t)a->npend * sizeof(a->pend[0]));
    n = nx_ancs_req_notif(a->cur_uid, req);
    nx_ancs_rsp_expect_notif(&a->rsp, a->cur_uid, 3);
    a->state = NX_ANCS_WAIT_NOTIF;
    a->t0 = now;
    if (a->ops.cp_write(a->ops.ctx, req, n) != 0) fail(a);
  }
}

void nx_ancs_on_ns(nx_ancs_t *a, const uint8_t *p, size_t len, uint32_t now_ms)
{
  nx_ancs_ns_t ev;
  uint8_t i;
  if (!a->on || nx_ancs_ns_decode(p, len, &ev) != NX_OK) return;
  if (ev.event == NX_ANCS_EV_ADDED) {
    if (ev.flags & (NX_ANCS_F_SILENT | NX_ANCS_F_PREEXISTING)) return;
    if (a->npend == NX_ANCS_PENDING) { /* full: the oldest waiting one goes */
      a->npend--;
      memmove(&a->pend[0], &a->pend[1], (size_t)a->npend * sizeof(a->pend[0]));
      a->n_failed++;
    }
    a->pend[a->npend].uid = ev.uid;
    a->pend[a->npend].cat = ev.category;
    a->npend++;
    issue(a, now_ms);
  } else if (ev.event == NX_ANCS_EV_REMOVED) {
    nx_ancs_note_t n;
    for (i = 0; i < a->npend; i++) {
      if (a->pend[i].uid != ev.uid) continue;
      a->npend--;
      memmove(&a->pend[i], &a->pend[i + 1], (size_t)(a->npend - i) * sizeof(a->pend[0]));
      break;
    }
    if (a->state != NX_ANCS_IDLE && a->cur_uid == ev.uid) a->cur_removed = 1;
    memset(&n, 0, sizeof(n));
    n.type = NX_ANCS_NOTE_REMOVED;
    n.category = ev.category;
    n.uid = ev.uid;
    if (a->ops.note) a->ops.note(a->ops.ctx, &n);
  }
  /* Modified: ignored (the card already shown stays as it is). */
}

void nx_ancs_on_ds(nx_ancs_t *a, const uint8_t *p, size_t len, uint32_t now_ms)
{
  int r;
  if (!a->on || a->state == NX_ANCS_IDLE) return;
  r = nx_ancs_rsp_feed(&a->rsp, p, len);
  if (r == NX_ANCS_RSP_DONE) {
    if (a->state == NX_ANCS_WAIT_NOTIF) notif_done(a, now_ms);
    else app_done(a);
  } else if (r == NX_ANCS_RSP_ERROR) {
    fail(a);
  }
  issue(a, now_ms);
}

void nx_ancs_cp_result(nx_ancs_t *a, uint8_t att_status, uint32_t now_ms)
{
  /* Success needs no action: the request completes when its Data Source
   * answer is complete (which may arrive before the write response). */
  if (!a->on || a->state == NX_ANCS_IDLE || att_status == 0) return;
  fail(a);
  issue(a, now_ms);
}

void nx_ancs_tick(nx_ancs_t *a, uint32_t now_ms)
{
  if (!a->on) return;
  if (a->state != NX_ANCS_IDLE && (uint32_t)(now_ms - a->t0) >= NX_ANCS_REQ_MS) fail(a);
  issue(a, now_ms);
}

/* ------------------------------------------------------------------------ */
size_t nx_adv_flags_uuid(uint8_t out[31], uint8_t ad_type, const uint8_t uuid_le[16])
{
  out[0] = 2;
  out[1] = 0x01;
  out[2] = 0x06; /* LE General Discoverable, BR/EDR not supported */
  out[3] = 17;
  out[4] = ad_type;
  memcpy(out + 5, uuid_le, 16);
  return 21;
}
