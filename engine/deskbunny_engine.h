/*
 * DeskBunny face engine: public API.
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

/* Drawing blob v2 (docs/ARCHITECTURE.md): composited by the firmware, not the
 * engine. The slot size is per firmware image (header bytes 14..15). */
#define DB_ART_MAGIC "DBART001"
#define DB_ART_VERSION 2
#define DB_ART_HEADER 16
#define DB_ART_FRAMES_MAX 10
#define DB_ART_OFF 0
#define DB_ART_REPLACE 1
#define DB_ART_OVER 2

/* What db_art_frame does with each decoded byte. */
#define DB_ART_SKIP 0 /* nothing: just step over the frame */
#define DB_ART_XOR  1 /* fb ^= byte (replace mode: frame 0 onto a clear fb, then deltas) */
#define DB_ART_OR   2 /* fb |= byte (over mode) */

/* Reads byte `off` of the drawing slot (flash on the board, RAM on the host). */
typedef uint8_t (*db_art_read_t)(uint16_t off);

enum db_mood {
  DB_MOOD_NEUTRAL = 0,
  DB_MOOD_HAPPY,
  DB_MOOD_ANGRY,
  DB_MOOD_SAD,
  DB_MOOD_SLEEPY,
  DB_MOOD_SURPRISED,
  DB_MOOD_LOVE,
  DB_MOOD_SUSPICIOUS,
  DB_MOOD_EXCITED,
  DB_MOOD_BORED,
  DB_MOOD_SCARED,
  DB_MOOD_DIZZY,
  DB_MOOD_WINK,
  DB_MOOD_LAUGH,
  DB_MOOD_CRYING,
  DB_MOOD_SKEPTICAL,
  DB_MOOD_SMUG,
  DB_MOOD_EVIL,
  DB_MOOD_WORRIED,
  DB_MOOD_SHY,
  DB_MOOD_SQUINT,
  DB_MOOD_DEAD,
  DB_MOOD_ASLEEP,
  DB_MOOD_STARRY,
  DB_MOOD_HYPNO,
  DB_MOOD_GLITCH,
  DB_MOOD_SCAN,
  DB_MOOD_EYEROLL,
  DB_MOOD_CONFUSED,
  DB_MOOD_DETERMINED,
  DB_MOOD_COUNT
};

enum db_overlay {
  DB_OVERLAY_NONE = 0,
  DB_OVERLAY_HEARTS,
  DB_OVERLAY_ZZZ,
  DB_OVERLAY_SWEAT,
  DB_OVERLAY_ANGER,
  DB_OVERLAY_TEARS,
  DB_OVERLAY_SPARKLES,
  DB_OVERLAY_QUESTION,
  DB_OVERLAY_COUNT
};

#define DB_FLAG_BLINK      0x01
#define DB_FLAG_LOOK       0x02
#define DB_FLAG_BREATHE    0x04
#define DB_FLAG_CYCLOPS    0x08
#define DB_FLAG_INVERT     0x10
#define DB_FLAG_ROTATE180  0x20
#define DB_FLAG_MOUTH      0x40

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

/* Decode one PackBits frame (exactly DB_FB_SIZE bytes) that starts at slot
 * offset *pos, never reading at or past `end`. On success *pos moves to the
 * next frame and 0 is returned; a corrupt stream returns -1 (fb may be
 * half-written, *pos unchanged). Token t < 128: t + 1 literal bytes follow;
 * t >= 128: the next byte repeats t - 126 times (2..129). */
int db_art_frame(db_art_read_t rd, uint16_t end, uint16_t *pos, uint8_t op, uint8_t fb[DB_FB_SIZE]);

/* Fill with the built-in default (neutral, blink + look + breathe). */
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

/* Clawd: the crab mascot of the Claude screen (live link, docs/ARCHITECTURE.md).
 * Stateless: the frame is a pure function of (anim, expr, now_ms). Clears fb
 * and draws above DB_CLAWD_GROUND, so rows below it stay free for a caption. */
enum db_clawd_anim {
  DB_CLAWD_IDLE = 0, /* stands, bobs, blinks */
  DB_CLAWD_WALK,     /* walks left and right across the screen */
  DB_CLAWD_JUMP,     /* hops with arms up and sparkles */
  DB_CLAWD_SHAKE,    /* jitters with a question mark over its head */
  DB_CLAWD_SLEEP,    /* sits with legs tucked, eyes shut, zzz */
  DB_CLAWD_WORK,     /* stands still, arms tapping, eyes down */
  DB_CLAWD_ANIM_COUNT
};

enum db_clawd_expr {
  DB_CLAWD_OPEN = 0, /* tall eyes, blinks */
  DB_CLAWD_HAPPY,    /* ^ ^ */
  DB_CLAWD_CLOSED,   /* - - */
  DB_CLAWD_WIDE,     /* big eyes with pupils, blinks */
  DB_CLAWD_SQUINT,   /* > < */
  DB_CLAWD_DEAD,     /* x x */
  DB_CLAWD_WORRIED,  /* small eyes, raised inner brows, blinks */
  DB_CLAWD_FOCUS,    /* tall eyes glancing left and right, blinks */
  DB_CLAWD_EXPR_COUNT
};

#define DB_CLAWD_GROUND 55 /* first row below the feet */

/* Unknown anim/expr values draw as IDLE/OPEN. */
void db_clawd_render(uint8_t anim, uint8_t expr, uint32_t now_ms, uint8_t fb[DB_FB_SIZE]);

#ifdef __cplusplus
}
#endif

#endif /* DESKBUNNY_ENGINE_H */
