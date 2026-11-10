// nixxa: persistent storage. See nx_store.h.
#include "nx_store.h"

#include <Arduino.h>
#include <FS.h>
#include <LittleFS.h>
#include <Preferences.h>
#include <string.h>

#include "deskbunny_engine.h"

static const char *NVS_NS = "nixxa";
static uint16_t usedMask;
static char names[NX_SLOTS][13];

// Files are written to a temporary name, then renamed over the old one, so a
// power cut leaves either the old or the new file, never half of one.
static bool writeAtomic(const char *path, const uint8_t *a, size_t alen, const uint8_t *b, size_t blen)
{
  char tmp[24];
  snprintf(tmp, sizeof(tmp), "%s.tmp", path);
  File f = LittleFS.open(tmp, FILE_WRITE);
  if (!f) return false;
  bool ok = f.write(a, alen) == alen && (blen == 0 || f.write(b, blen) == blen);
  f.close();
  if (!ok) {
    LittleFS.remove(tmp);
    return false;
  }
  if (!LittleFS.rename(tmp, path)) {
    LittleFS.remove(path);
    if (!LittleFS.rename(tmp, path)) {
      LittleFS.remove(tmp);
      return false;
    }
  }
  return true;
}

static void slotPath(uint8_t slot, char out[12]) { snprintf(out, 12, "/s/%02u.bin", (unsigned)slot); }

static void cacheName(uint8_t slot, const uint8_t face[NX_FACE_SIZE])
{
  db_face_t f;
  db_face_decode(face, &f); // bad blob -> default face name
  memcpy(names[slot], f.name, 13);
}

bool nxStoreBegin()
{
  // formatOnFail: a blank or corrupt partition becomes an empty library.
  if (!LittleFS.begin(true)) return false;
  if (!LittleFS.exists("/s")) LittleFS.mkdir("/s");
  usedMask = 0;
  for (uint8_t i = 0; i < NX_SLOTS; i++) {
    char p[12];
    slotPath(i, p);
    names[i][0] = '\0';
    File f = LittleFS.open(p, FILE_READ);
    if (!f) continue;
    uint8_t face[NX_FACE_SIZE];
    bool ok = f.size() >= NX_FACE_SIZE && f.read(face, NX_FACE_SIZE) == NX_FACE_SIZE;
    f.close();
    if (!ok) continue;
    usedMask |= (uint16_t)(1u << i);
    cacheName(i, face);
  }
  return true;
}

// ---- NVS --------------------------------------------------------------------
bool nxNvsLoadK(uint8_t k[16])
{
  Preferences p;
  if (!p.begin(NVS_NS, true)) return false; // read-only open fails when the namespace is new
  bool ok = p.getBytesLength("k") == 16 && p.getBytes("k", k, 16) == 16;
  p.end();
  return ok;
}

bool nxNvsSaveK(const uint8_t k[16])
{
  Preferences p;
  if (!p.begin(NVS_NS, false)) return false;
  bool ok = p.putBytes("k", k, 16) == 16;
  p.end();
  return ok;
}

bool nxNvsPaired()
{
  Preferences p;
  if (!p.begin(NVS_NS, true)) return false;
  bool v = p.getUChar("paired", 0) != 0;
  p.end();
  return v;
}

bool nxNvsSetPaired(bool paired)
{
  Preferences p;
  if (!p.begin(NVS_NS, false)) return false;
  bool ok = p.putUChar("paired", paired ? 1 : 0) == 1;
  p.end();
  return ok;
}

// ---- slots ------------------------------------------------------------------
uint16_t nxSlotsUsedMask() { return usedMask; }

void nxSlotName(uint8_t slot, char out[13])
{
  if (slot >= NX_SLOTS || !(usedMask & (1u << slot))) {
    out[0] = '\0';
    return;
  }
  memcpy(out, names[slot], 13);
}

bool nxSlotRead(uint8_t slot, uint8_t face[NX_FACE_SIZE], uint8_t *art, uint16_t *artLen)
{
  *artLen = 0;
  if (slot >= NX_SLOTS) return false;
  char p[12];
  slotPath(slot, p);
  File f = LittleFS.open(p, FILE_READ);
  if (!f) return false;
  size_t size = f.size();
  bool ok = size >= NX_FACE_SIZE && size <= NX_FACE_SIZE + NX_ART_MAX && f.read(face, NX_FACE_SIZE) == NX_FACE_SIZE;
  if (ok && size > NX_FACE_SIZE) {
    size_t n = size - NX_FACE_SIZE;
    ok = f.read(art, n) == n;
    if (ok) *artLen = (uint16_t)n;
  }
  f.close();
  if (!ok) *artLen = 0;
  return ok;
}

int nxSlotWrite(uint8_t slot, const uint8_t face[NX_FACE_SIZE], const uint8_t *art, uint16_t artLen)
{
  if (slot >= NX_SLOTS) return NX_BAD_SLOT;
  // Room for the new file next to the old one (it is replaced by rename),
  // plus a few blocks of LittleFS metadata headroom.
  size_t need = NX_FACE_SIZE + artLen + 4 * 4096;
  size_t total = LittleFS.totalBytes(), used = LittleFS.usedBytes();
  if (used + need > total) return NX_NO_SPACE;
  char p[12];
  slotPath(slot, p);
  if (!writeAtomic(p, face, NX_FACE_SIZE, art, artLen)) return NX_NO_SPACE;
  usedMask |= (uint16_t)(1u << slot);
  cacheName(slot, face);
  return NX_OK;
}

bool nxSlotDelete(uint8_t slot)
{
  if (slot >= NX_SLOTS) return false;
  char p[12];
  slotPath(slot, p);
  if (LittleFS.exists(p) && !LittleFS.remove(p)) return false;
  usedMask &= (uint16_t)~(1u << slot);
  names[slot][0] = '\0';
  return true;
}

// ---- state ------------------------------------------------------------------
// /state.bin: "NXS1", activeSlot u8, dnd u8, then the config in the
// SET_NOTIF_CONFIG wire format.
static const uint8_t STATE_MAGIC[4] = { 'N', 'X', 'S', '1' };

bool nxStateLoad(uint8_t *active, uint8_t *dnd, nx_notif_cfg_t *cfg)
{
  static uint8_t buf[6 + NX_CFG_WIRE_MAX];
  File f = LittleFS.open("/state.bin", FILE_READ);
  if (!f) return false;
  size_t n = f.read(buf, sizeof(buf));
  f.close();
  if (n < 6 || memcmp(buf, STATE_MAGIC, 4) != 0 || buf[4] >= NX_SLOTS) return false;
  if (nx_cfg_parse(buf + 6, n - 6, DB_MOOD_COUNT, cfg) != NX_OK) return false;
  *active = buf[4];
  *dnd = buf[5] ? 1 : 0;
  return true;
}

bool nxStateSave(uint8_t active, uint8_t dnd, const nx_notif_cfg_t *cfg)
{
  static uint8_t buf[6 + NX_CFG_WIRE_MAX];
  memcpy(buf, STATE_MAGIC, 4);
  buf[4] = active;
  buf[5] = dnd;
  size_t n = 6 + nx_cfg_write(cfg, buf + 6);
  return writeAtomic("/state.bin", buf, n, NULL, 0);
}

// ---- clock ------------------------------------------------------------------
bool nxClockLoad(uint8_t *style)
{
  File f = LittleFS.open("/clock.bin", FILE_READ);
  if (!f) return false;
  uint8_t b = 0;
  size_t n = f.read(&b, 1);
  f.close();
  if (n != 1 || b > NX_CLOCK_STYLES) return false;
  *style = b;
  return true;
}

bool nxClockSave(uint8_t style)
{
  return writeAtomic("/clock.bin", &style, 1, NULL, 0);
}
