// nixxa: persistent storage. LittleFS (the "spiffs" partition) holds the
// library slots and /state.bin; NVS namespace "nixxa" holds the secret K and
// the paired flag, so a desktop reflash (which erases NVS) resets the pairing
// but keeps the library. Contract: docs/ARCHITECTURE.md, "ESP32 + BLE".
//
// Not thread-safe: only setup() and the protocol task call these.
#pragma once

#include <stdint.h>
#include "nx_clock.h"
#include "nx_core.h"

bool nxStoreBegin();

// NVS
bool nxNvsLoadK(uint8_t k[16]);   // false when there is none yet
bool nxNvsSaveK(const uint8_t k[16]);
bool nxNvsPaired();
bool nxNvsSetPaired(bool paired);

// Library slots: /s/NN.bin = face blob v1 (64 B) + optional drawing blob v2.
uint16_t nxSlotsUsedMask();
void nxSlotName(uint8_t slot, char out[13]); // "" when the slot is empty
// artLen = 0 when the slot has no drawing. art must hold NX_ART_MAX bytes.
bool nxSlotRead(uint8_t slot, uint8_t face[NX_FACE_SIZE], uint8_t *art, uint16_t *artLen);
int nxSlotWrite(uint8_t slot, const uint8_t face[NX_FACE_SIZE], const uint8_t *art, uint16_t artLen); // NX_OK / NX_NO_SPACE
bool nxSlotDelete(uint8_t slot);

// /state.bin: active slot, DND and the notification config.
bool nxStateLoad(uint8_t *active, uint8_t *dnd, nx_notif_cfg_t *cfg);
bool nxStateSave(uint8_t active, uint8_t dnd, const nx_notif_cfg_t *cfg);

// /clock.bin: the idle clock style (0 = the face; see SET_CLOCK, op 0x25).
bool nxClockLoad(uint8_t *style);
bool nxClockSave(uint8_t style);
