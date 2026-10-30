/*
 * DeskBunny engine (drone animations): public API.
 * Shared verbatim by the Arduino firmware and the desktop simulator (via FFI).
 * C99, no malloc, no stdio. Contract: docs/ARCHITECTURE.md.
 */
#ifndef DESKBUNNY_ENGINE_H
#define DESKBUNNY_ENGINE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define DB_W 128
#define DB_H 64
#define DB_FB_SIZE 1024 /* SSD1306 page layout: fb[(y>>3)*128 + x], bit (y&7) */
#define DB_BLOB_SIZE 64
#define DB_MAGIC "DBFACE01"
#define DB_BLOB_VERSION 1

/* Drawing blob v1 (docs/ARCHITECTURE.md): composited by the firmware, not the engine. */
#define DB_ART_MAGIC "DBART001"
#define DB_ART_VERSION 1
#define DB_ART_HEADER 16
#define DB_ART_FRAMES 4
#define DB_ART_SIZE (DB_ART_HEADER + DB_ART_FRAMES * DB_FB_SIZE)
#define DB_ART_OFF 0
#define DB_ART_REPLACE 1
#define DB_ART_OVER 2

/* Flight animation, stored in the blob's mood byte. */
enum db_mood {
  DB_MOOD_HOVER = 0,
  DB_MOOD_PATROL,
  DB_MOOD_TAKEOFF,
  DB_MOOD_FLIP,
  DB_MOOD_ORBIT,
  DB_MOOD_FORMATION,
  DB_MOOD_DELIVERY,
  DB_MOOD_RACE,
  DB_MOOD_SCAN,
  DB_MOOD_SWARM,
  DB_MOOD_TOPDOWN,
  DB_MOOD_CHARGE,
  DB_MOOD_COUNT
};

/* Sky background, stored in the blob's overlay byte. */
enum db_overlay {
  DB_OVERLAY_NONE = 0,
  DB_OVERLAY_CLOUDS,
  DB_OVERLAY_STARS,
  DB_OVERLAY_RAIN,
  DB_OVERLAY_CITY,
  DB_OVERLAY_HILLS,
  DB_OVERLAY_WIND,
  DB_OVERLAY_GRID,
  DB_OVERLAY_COUNT
};

/* Flag bits keep their face-era names; drone meaning on the right. */
#define DB_FLAG_BLINK      0x01 /* strobe: nav lights flash every blink_min */
#define DB_FLAG_LOOK       0x02 /* drift around within look_x/look_y */
#define DB_FLAG_BREATHE    0x04 /* hover bob */
#define DB_FLAG_CYCLOPS    0x08 /* camera gimbal under the body */
#define DB_FLAG_INVERT     0x10
#define DB_FLAG_ROTATE180  0x20
#define DB_FLAG_MOUTH      0x40 /* ground shadow */

enum db_cycle {
  DB_CYCLE_OFF = 0,
  DB_CYCLE_SEQUENCE,
  DB_CYCLE_RANDOM,
  DB_CYCLE_COUNT
};

#define DB_CYCLE_MAX          12   /* moods in the cycle list */
#define DB_CYCLE_FLAG_OVERLAY 0x01 /* overlay follows the mood */

/* Decoded face (field meanings and ranges: docs/ARCHITECTURE.md). */
typedef struct db_face {
  uint8_t mood;
  char name[13]; /* NUL-terminated */
  uint8_t eye_w, eye_h, eye_r, eye_gap;
  int8_t eye_y;
  uint8_t flags;
  uint8_t blink_min, blink_max; /* x100 ms */
  uint8_t look_min, look_max;   /* x100 ms */
  uint8_t look_x, look_y;
  uint8_t speed;
  uint8_t contrast;
  uint8_t overlay;
  uint8_t seed;
  uint8_t cycle;       /* enum db_cycle */
  uint8_t cycle_secs;  /* seconds per mood, 1..255 */
  uint8_t cycle_count; /* 0 = all moods */
  uint8_t cycle_flags;
  uint8_t cycle_moods[DB_CYCLE_MAX];
} db_face_t;

/* Opaque-ish runtime state. Callers allocate it; only the engine touches fields.
 * Must stay <= 96 bytes on AVR. */
typedef struct db_engine {
  db_face_t face;
  uint32_t rng;
  uint32_t last_ms;
  uint32_t next_blink_ms;
  uint32_t blink_start_ms;
  uint32_t next_look_ms;
  uint32_t cycle_at; /* time of the next mood change */
  int16_t look_cur_x, look_cur_y; /* fixed point 8.8 */
  int8_t look_tgt_x, look_tgt_y;
  uint8_t blinking;
  uint8_t cycle_idx; /* position in the cycle list (sequence mode) */
  uint8_t pad[2];
} db_engine_t;

uint16_t db_crc16(const uint8_t *data, size_t len);

/* Fill with the built-in default (hover, strobe + drift + bob). */
void db_face_default(db_face_t *out);

/* Decode a 64-byte blob. Returns 0 on success; negative on bad magic (-1),
 * version (-2) or CRC (-3). On failure *out is set to db_face_default(). */
int db_face_decode(const uint8_t blob[DB_BLOB_SIZE], db_face_t *out);

/* Encode a face into a 64-byte blob, including magic and CRC. */
void db_face_encode(const db_face_t *face, uint8_t blob[DB_BLOB_SIZE]);

void db_engine_init(db_engine_t *e, const db_face_t *face, uint32_t seed);

/* Render the frame for absolute time now_ms (monotonic, starts near 0).
 * Clears and fully redraws fb. Deterministic for the same init + time sequence.
 * DB_FLAG_INVERT and DB_FLAG_ROTATE180 are applied to fb here. */
void db_engine_render(db_engine_t *e, uint32_t now_ms, uint8_t fb[DB_FB_SIZE]);

#ifdef __cplusplus
}
#endif

#endif /* DESKBUNNY_ENGINE_H */
