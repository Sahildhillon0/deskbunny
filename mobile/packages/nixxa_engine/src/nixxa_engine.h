/*
 * nixxa_engine: the desktop simulator's compositing (crates/deskbunny-core
 * src/engine.rs + drawing.rs) on top of the C face engine, for Dart FFI.
 * deskbunny_engine.{c,h} are copies of engine/ (mobile/tool/sync_engine.sh).
 */
#ifndef NIXXA_ENGINE_H
#define NIXXA_ENGINE_H

#include <stdint.h>

#if _WIN32
#define FFI_PLUGIN_EXPORT __declspec(dllexport)
#else
#define FFI_PLUGIN_EXPORT __attribute__((visibility("default"))) __attribute__((used))
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef struct nx_sim nx_sim;

/* A simulator with the default face and no drawing. NULL if out of memory. */
FFI_PLUGIN_EXPORT nx_sim *nx_sim_new(uint32_t seed);
FFI_PLUGIN_EXPORT void nx_sim_free(nx_sim *s);

/* Set the face from a 64-byte face blob v1 and restart the engine. Returns
 * db_face_decode's code (0 ok; -1 magic, -2 version, -3 CRC: the face is left
 * unchanged on error). */
FFI_PLUGIN_EXPORT int32_t nx_sim_set_face(nx_sim *s, const uint8_t *blob);

/* Set the drawing from a drawing blob v2 (NULL/len 0 = none) and restart the
 * engine. Returns 1 when a drawing is shown, 0 when the blob is "off", -1 when
 * it is invalid (then no drawing is shown, as on the device). */
FFI_PLUGIN_EXPORT int32_t nx_sim_set_drawing(nx_sim *s, const uint8_t *blob, uint32_t len);

/* Render the frame at t_ms into the sim's 1024-byte buffer (page layout) and
 * return it. Valid until the next call on this sim. */
FFI_PLUGIN_EXPORT const uint8_t *nx_sim_render(nx_sim *s, uint32_t t_ms);

/* Clock style (1..20, 0 = the face is not a clock) drawn for hour:minute:second
 * into out_fb, a 1024-byte page-layout frame (firmware nx_clock.h). */
FFI_PLUGIN_EXPORT void nx_clock_preview(uint8_t style, uint8_t hour, uint8_t minute, uint8_t second,
                                        uint8_t *out_fb);

/* Short English name of a clock style, e.g. "Big digits". Never NULL; the
 * string is static. */
FFI_PLUGIN_EXPORT const char *nx_clock_style_name(uint8_t style);

#ifdef __cplusplus
}
#endif

#endif
