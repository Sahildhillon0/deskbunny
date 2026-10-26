/*
 * Host renderer for the DeskBunny engine.
 *
 *   render <face> <t_start_ms> <t_end_ms> <step_ms> <outdir>
 *       writes <outdir>/frame_NNNNN.pbm per step and <outdir>/strip.pbm
 *       (all frames stacked vertically, 1 px separator).
 *   render sheet <out.pbm> [t_ms...]
 *       contact sheet: one row per mood (with mouth and a fitting overlay),
 *       one column per time (default 600 1400 2200 3000 ms).
 *   render blob <face> <out.bin>     encode a face to a 64-byte blob
 *   render blobhdr                   print firmware/deskbunny/db_default_blob.h
 *
 * <face> is a path to a 64-byte blob (*.bin) or a spec:
 *   mood[:overlay][+mouth][+cyclops][+invert][+rotate][+noblink][+nolook]
 *   e.g. "happy", "love:hearts+mouth", "sleepy:zzz".
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "../deskbunny_engine.h"

static const char *const k_moods[DB_MOOD_COUNT] = {
  "neutral", "happy", "angry", "sad", "sleepy", "surprised",
  "love", "suspicious", "excited", "bored", "scared", "dizzy"
};
static const char *const k_overlays[DB_OVERLAY_COUNT] = {
  "none", "hearts", "zzz", "sweat", "anger", "tears", "sparkles", "question"
};

static int lookup(const char *s, size_t n, const char *const *tab, int count)
{
  int i;
  for (i = 0; i < count; i++)
    if (strlen(tab[i]) == n && strncmp(tab[i], s, n) == 0) return i;
  return -1;
}

static int load_face(const char *spec, db_face_t *f)
{
  size_t n = strlen(spec);
  if (n > 4 && strcmp(spec + n - 4, ".bin") == 0) {
    uint8_t blob[DB_BLOB_SIZE];
    FILE *fp = fopen(spec, "rb");
    int rc;
    if (!fp || fread(blob, 1, sizeof(blob), fp) != sizeof(blob)) {
      fprintf(stderr, "cannot read 64 bytes from %s\n", spec);
      if (fp) fclose(fp);
      return -1;
    }
    fclose(fp);
    rc = db_face_decode(blob, f);
    if (rc != 0) fprintf(stderr, "warning: blob rejected (%d), using default face\n", rc);
    return 0;
  } else {
    const char *p = spec, *end;
    int m;
    db_face_default(f);
    end = p + strcspn(p, ":+");
    m = lookup(p, (size_t)(end - p), k_moods, DB_MOOD_COUNT);
    if (m < 0) { fprintf(stderr, "unknown mood in '%s'\n", spec); return -1; }
    f->mood = (uint8_t)m;
    p = end;
    while (*p) {
      char sep = *p++;
      end = p + strcspn(p, ":+");
      if (sep == ':') {
        int o = lookup(p, (size_t)(end - p), k_overlays, DB_OVERLAY_COUNT);
        if (o < 0) { fprintf(stderr, "unknown overlay in '%s'\n", spec); return -1; }
        f->overlay = (uint8_t)o;
      } else if (!strncmp(p, "mouth", 5)) f->flags |= DB_FLAG_MOUTH;
      else if (!strncmp(p, "cyclops", 7)) f->flags |= DB_FLAG_CYCLOPS;
      else if (!strncmp(p, "invert", 6)) f->flags |= DB_FLAG_INVERT;
      else if (!strncmp(p, "rotate", 6)) f->flags |= DB_FLAG_ROTATE180;
      else if (!strncmp(p, "noblink", 7)) f->flags &= (uint8_t)~DB_FLAG_BLINK;
      else if (!strncmp(p, "nolook", 6)) f->flags &= (uint8_t)~DB_FLAG_LOOK;
      else { fprintf(stderr, "unknown option in '%s'\n", spec); return -1; }
      p = end;
    }
  }
  return 0;
}

/* 1-bit canvas, row-major, 1 = lit. */
typedef struct canvas {
  int w, h;
  unsigned char *px;
} canvas_t;

static int canvas_new(canvas_t *c, int w, int h)
{
  c->w = w;
  c->h = h;
  c->px = (unsigned char *)calloc((size_t)w * (size_t)h, 1);
  return c->px ? 0 : -1;
}

static void blit(canvas_t *c, const uint8_t *fb, int ox, int oy)
{
  int x, y;
  for (y = 0; y < DB_H; y++)
    for (x = 0; x < DB_W; x++)
      c->px[(size_t)(oy + y) * (size_t)c->w + (size_t)(ox + x)] =
          (unsigned char)((fb[(y >> 3) * DB_W + x] >> (y & 7)) & 1);
}

/* Separator lines are drawn lit (the frames themselves are mostly dark). */
static void hsep(canvas_t *c, int y)
{
  int x;
  for (x = 0; x < c->w; x++) c->px[(size_t)y * (size_t)c->w + (size_t)x] = 1;
}

static void vsep(canvas_t *c, int x)
{
  int y;
  for (y = 0; y < c->h; y++) c->px[(size_t)y * (size_t)c->w + (size_t)x] = 1;
}

static int write_pbm(const char *path, const canvas_t *c)
{
  FILE *fp = fopen(path, "wb");
  int x, y;
  if (!fp) { perror(path); return -1; }
  fprintf(fp, "P4\n%d %d\n", c->w, c->h);
  for (y = 0; y < c->h; y++) {
    for (x = 0; x < c->w; x += 8) {
      unsigned char b = 0;
      int k;
      for (k = 0; k < 8 && x + k < c->w; k++)
        if (c->px[(size_t)y * (size_t)c->w + (size_t)(x + k)]) b |= (unsigned char)(0x80 >> k);
      fputc(b, fp);
    }
  }
  return fclose(fp) == 0 ? 0 : -1;
}

static int cmd_frames(int argc, char **argv)
{
  db_face_t face;
  db_engine_t eng;
  uint8_t fb[DB_FB_SIZE];
  canvas_t strip, one;
  long t0, t1, step, t;
  int n, i;
  char path[1024];
  if (argc != 6) return 2;
  if (load_face(argv[1], &face) != 0) return 1;
  t0 = atol(argv[2]);
  t1 = atol(argv[3]);
  step = atol(argv[4]);
  if (step <= 0 || t1 < t0 || t0 < 0) { fprintf(stderr, "bad time range\n"); return 1; }
  n = (int)((t1 - t0) / step) + 1;
  if (n > 10000) { fprintf(stderr, "too many frames (%d)\n", n); return 1; }
  if (canvas_new(&strip, DB_W, n * (DB_H + 1) - 1) || canvas_new(&one, DB_W, DB_H)) return 1;
  db_engine_init(&eng, &face, 1);
  /* run the clock from 0 so timers behave as on the device */
  for (t = 0; t < t0; t += 33) db_engine_render(&eng, (uint32_t)t, fb);
  for (i = 0, t = t0; i < n; i++, t += step) {
    db_engine_render(&eng, (uint32_t)t, fb);
    memset(one.px, 0, (size_t)DB_W * DB_H);
    blit(&one, fb, 0, 0);
    snprintf(path, sizeof(path), "%s/frame_%05d.pbm", argv[5], i);
    if (write_pbm(path, &one)) return 1;
    blit(&strip, fb, 0, i * (DB_H + 1));
    if (i) hsep(&strip, i * (DB_H + 1) - 1);
  }
  snprintf(path, sizeof(path), "%s/strip.pbm", argv[5]);
  if (write_pbm(path, &strip)) return 1;
  printf("%d frames -> %s\n", n, argv[5]);
  free(strip.px);
  free(one.px);
  return 0;
}

static int cmd_sheet(int argc, char **argv)
{
  static const uint8_t ov[DB_MOOD_COUNT] = {
    DB_OVERLAY_NONE, DB_OVERLAY_SPARKLES, DB_OVERLAY_ANGER, DB_OVERLAY_TEARS,
    DB_OVERLAY_ZZZ, DB_OVERLAY_NONE, DB_OVERLAY_HEARTS, DB_OVERLAY_QUESTION,
    DB_OVERLAY_SPARKLES, DB_OVERLAY_NONE, DB_OVERLAY_SWEAT, DB_OVERLAY_NONE
  };
  long times[16] = { 600, 1400, 2200, 3000 };
  int nt = 4, m, k;
  canvas_t c;
  if (argc < 2) return 2;
  if (argc > 2) {
    nt = argc - 2 > 16 ? 16 : argc - 2;
    for (k = 0; k < nt; k++) times[k] = atol(argv[2 + k]);
  }
  if (canvas_new(&c, nt * (DB_W + 2) - 2, DB_MOOD_COUNT * (DB_H + 2) - 2)) return 1;
  for (m = 0; m < DB_MOOD_COUNT; m++) {
    db_face_t face;
    db_face_default(&face);
    face.mood = (uint8_t)m;
    face.overlay = ov[m];
    face.flags = DB_FLAG_BREATHE | DB_FLAG_MOUTH | DB_FLAG_LOOK;
    for (k = 0; k < nt; k++) {
      db_engine_t eng;
      uint8_t fb[DB_FB_SIZE];
      long t;
      db_engine_init(&eng, &face, 7);
      for (t = 0; t < times[k]; t += 33) db_engine_render(&eng, (uint32_t)t, fb);
      db_engine_render(&eng, (uint32_t)times[k], fb);
      blit(&c, fb, k * (DB_W + 2), m * (DB_H + 2));
    }
    if (m) { hsep(&c, m * (DB_H + 2) - 2); hsep(&c, m * (DB_H + 2) - 1); }
  }
  for (k = 1; k < nt; k++) { vsep(&c, k * (DB_W + 2) - 2); vsep(&c, k * (DB_W + 2) - 1); }
  if (write_pbm(argv[1], &c)) return 1;
  printf("sheet %dx%d -> %s\n", c.w, c.h, argv[1]);
  free(c.px);
  return 0;
}

static int cmd_blob(int argc, char **argv)
{
  db_face_t face;
  uint8_t blob[DB_BLOB_SIZE];
  FILE *fp;
  if (argc != 3) return 2;
  if (load_face(argv[1], &face) != 0) return 1;
  db_face_encode(&face, blob);
  fp = fopen(argv[2], "wb");
  if (!fp || fwrite(blob, 1, sizeof(blob), fp) != sizeof(blob)) { perror(argv[2]); return 1; }
  return fclose(fp) == 0 ? 0 : 1;
}

static int cmd_blobhdr(void)
{
  db_face_t face;
  uint8_t blob[DB_BLOB_SIZE];
  int i;
  db_face_default(&face);
  db_face_encode(&face, blob);
  printf("/* Generated by `engine/build/render blobhdr`: db_face_encode(db_face_default()).\n"
         " * The firmware's DB_FACE_BLOB initializer; engine/tests checks it stays in sync. */\n"
         "#ifndef DB_DEFAULT_BLOB_H\n#define DB_DEFAULT_BLOB_H\n\n#define DB_DEFAULT_BLOB_INIT { \\\n");
  for (i = 0; i < DB_BLOB_SIZE; i++)
    printf("%s0x%02X%s", i % 8 == 0 ? "  " : "", blob[i],
           i == DB_BLOB_SIZE - 1 ? " \\\n" : (i % 8 == 7 ? ", \\\n" : ", "));
  printf("}\n\n#endif\n");
  return 0;
}

int main(int argc, char **argv)
{
  int rc = 2;
  if (argc >= 2 && strcmp(argv[1], "sheet") == 0) rc = cmd_sheet(argc - 1, argv + 1);
  else if (argc >= 2 && strcmp(argv[1], "blob") == 0) rc = cmd_blob(argc - 1, argv + 1);
  else if (argc == 2 && strcmp(argv[1], "blobhdr") == 0) rc = cmd_blobhdr();
  else if (argc == 6) rc = cmd_frames(argc, argv);
  if (rc == 2)
    fprintf(stderr,
            "usage: render <mood[:overlay][+mouth...]|blob.bin> <t_start_ms> <t_end_ms> <step_ms> <outdir>\n"
            "       render sheet <out.pbm> [t_ms...]\n"
            "       render blob <face> <out.bin>\n"
            "       render blobhdr\n");
  return rc;
}
