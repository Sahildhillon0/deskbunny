/* nixxa: SHA-256 / HMAC-SHA256. See nx_sha256.h. */
#include "nx_sha256.h"

#include <string.h>

static const uint32_t K256[64] = {
  0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u, 0x3956c25bu, 0x59f111f1u, 0x923f82a4u, 0xab1c5ed5u,
  0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u, 0x72be5d74u, 0x80deb1feu, 0x9bdc06a7u, 0xc19bf174u,
  0xe49b69c1u, 0xefbe4786u, 0x0fc19dc6u, 0x240ca1ccu, 0x2de92c6fu, 0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau,
  0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u, 0xc6e00bf3u, 0xd5a79147u, 0x06ca6351u, 0x14292967u,
  0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu, 0x53380d13u, 0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u,
  0xa2bfe8a1u, 0xa81a664bu, 0xc24b8b70u, 0xc76c51a3u, 0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u,
  0x19a4c116u, 0x1e376c08u, 0x2748774cu, 0x34b0bcb5u, 0x391c0cb3u, 0x4ed8aa4au, 0x5b9cca4fu, 0x682e6ff3u,
  0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u, 0x90befffau, 0xa4506cebu, 0xbef9a3f7u, 0xc67178f2u
};

#define ROR(x, n) (((x) >> (n)) | ((x) << (32 - (n))))

static void block(nx_sha256_t *s, const uint8_t *p)
{
  uint32_t w[64], a, b, c, d, e, f, g, h, t1, t2;
  int i;
  for (i = 0; i < 16; i++)
    w[i] = ((uint32_t)p[4 * i] << 24) | ((uint32_t)p[4 * i + 1] << 16) | ((uint32_t)p[4 * i + 2] << 8) |
           (uint32_t)p[4 * i + 3];
  for (i = 16; i < 64; i++) {
    uint32_t s0 = ROR(w[i - 15], 7) ^ ROR(w[i - 15], 18) ^ (w[i - 15] >> 3);
    uint32_t s1 = ROR(w[i - 2], 17) ^ ROR(w[i - 2], 19) ^ (w[i - 2] >> 10);
    w[i] = w[i - 16] + s0 + w[i - 7] + s1;
  }
  a = s->h[0]; b = s->h[1]; c = s->h[2]; d = s->h[3];
  e = s->h[4]; f = s->h[5]; g = s->h[6]; h = s->h[7];
  for (i = 0; i < 64; i++) {
    t1 = h + (ROR(e, 6) ^ ROR(e, 11) ^ ROR(e, 25)) + ((e & f) ^ (~e & g)) + K256[i] + w[i];
    t2 = (ROR(a, 2) ^ ROR(a, 13) ^ ROR(a, 22)) + ((a & b) ^ (a & c) ^ (b & c));
    h = g; g = f; f = e; e = d + t1;
    d = c; c = b; b = a; a = t1 + t2;
  }
  s->h[0] += a; s->h[1] += b; s->h[2] += c; s->h[3] += d;
  s->h[4] += e; s->h[5] += f; s->h[6] += g; s->h[7] += h;
}

void nx_sha256_init(nx_sha256_t *s)
{
  static const uint32_t iv[8] = { 0x6a09e667u, 0xbb67ae85u, 0x3c6ef372u, 0xa54ff53au,
                                  0x510e527fu, 0x9b05688cu, 0x1f83d9abu, 0x5be0cd19u };
  memcpy(s->h, iv, sizeof(iv));
  s->bits = 0;
  s->n = 0;
}

void nx_sha256_update(nx_sha256_t *s, const uint8_t *d, size_t len)
{
  s->bits += (uint64_t)len * 8u;
  while (len--) {
    s->buf[s->n++] = *d++;
    if (s->n == 64) {
      block(s, s->buf);
      s->n = 0;
    }
  }
}

void nx_sha256_final(nx_sha256_t *s, uint8_t out[32])
{
  uint64_t bits = s->bits;
  int i;
  s->buf[s->n++] = 0x80;
  if (s->n > 56) {
    while (s->n < 64) s->buf[s->n++] = 0;
    block(s, s->buf);
    s->n = 0;
  }
  while (s->n < 56) s->buf[s->n++] = 0;
  for (i = 0; i < 8; i++) s->buf[56 + i] = (uint8_t)(bits >> (56 - 8 * i));
  block(s, s->buf);
  for (i = 0; i < 8; i++) {
    out[4 * i] = (uint8_t)(s->h[i] >> 24);
    out[4 * i + 1] = (uint8_t)(s->h[i] >> 16);
    out[4 * i + 2] = (uint8_t)(s->h[i] >> 8);
    out[4 * i + 3] = (uint8_t)s->h[i];
  }
  memset(s, 0, sizeof(*s));
}

void nx_hmac_sha256(const uint8_t *key, size_t key_len, const uint8_t *msg, size_t msg_len,
                    uint8_t out[32])
{
  uint8_t k[64], pad[64], inner[32];
  nx_sha256_t s;
  size_t i;
  memset(k, 0, sizeof(k));
  if (key_len > 64) {
    nx_sha256_init(&s);
    nx_sha256_update(&s, key, key_len);
    nx_sha256_final(&s, k);
  } else {
    memcpy(k, key, key_len);
  }
  for (i = 0; i < 64; i++) pad[i] = (uint8_t)(k[i] ^ 0x36);
  nx_sha256_init(&s);
  nx_sha256_update(&s, pad, 64);
  nx_sha256_update(&s, msg, msg_len);
  nx_sha256_final(&s, inner);
  for (i = 0; i < 64; i++) pad[i] = (uint8_t)(k[i] ^ 0x5c);
  nx_sha256_init(&s);
  nx_sha256_update(&s, pad, 64);
  nx_sha256_update(&s, inner, 32);
  nx_sha256_final(&s, out);
  memset(k, 0, sizeof(k));
  memset(pad, 0, sizeof(pad));
  memset(inner, 0, sizeof(inner));
}
