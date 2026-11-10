/*
 * nixxa: SHA-256 and HMAC-SHA256 (FIPS 180-4, RFC 2104). Portable C99 so the
 * AUTH tag is computed by the same code on the device and in host tests.
 */
#ifndef NX_SHA256_H
#define NX_SHA256_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct nx_sha256 {
  uint32_t h[8];
  uint64_t bits;
  uint8_t buf[64];
  uint8_t n;
} nx_sha256_t;

void nx_sha256_init(nx_sha256_t *s);
void nx_sha256_update(nx_sha256_t *s, const uint8_t *d, size_t len);
void nx_sha256_final(nx_sha256_t *s, uint8_t out[32]);
void nx_hmac_sha256(const uint8_t *key, size_t key_len, const uint8_t *msg, size_t msg_len,
                    uint8_t out[32]);

#ifdef __cplusplus
}
#endif

#endif
