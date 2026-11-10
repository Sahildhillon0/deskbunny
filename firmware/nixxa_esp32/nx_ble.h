// nixxa: BLE peripheral (GATT service, LE Secure Connections bonding, owner-only
// advertising). Built on the core's bundled BLE library; the backend is fixed
// by the core's precompiled SDK: Bluedroid on the ESP32, NimBLE on the C3/S3.
// Where the library has no portable API (bond list, filter accept list, local
// privacy) the backend's own API is used.
//
// One connection at a time: advertising stops while a phone is connected.
// Contract: docs/ARCHITECTURE.md, "BLE GATT" and "Pairing and security".
#pragma once

#include <stddef.h>
#include <stdint.h>

#define NX_SVC_UUID  "4e580001-6e69-4878-a100-6e6978786100"
#define NX_INFO_UUID "4e580002-6e69-4878-a100-6e6978786100"
#define NX_RX_UUID   "4e580003-6e69-4878-a100-6e6978786100"
#define NX_TX_UUID   "4e580004-6e69-4878-a100-6e6978786100"

// Hooks implemented by the application. They run in the BLE host task and
// must not block (copy, post an event, return).
void nxAppOnConnect();
void nxAppOnDisconnect();
void nxAppOnEncrypted(bool ok);
void nxAppOnRx(const uint8_t *data, size_t len);
// ANCS client (iOS), see nxBleAncsStart. what: NX_ANCS_IN_*.
//   READY  status 1: ANCS found and Data Source subscribed (the Notification
//          Source subscription follows right after); 0: no usable ANCS.
//   NS/DS  a Notification Source / Data Source value (data, len).
//   CP     status = result of the last nxBleAncsWriteCp: 0 or an ATT error.
enum { NX_ANCS_IN_READY = 0, NX_ANCS_IN_NS = 1, NX_ANCS_IN_DS = 2, NX_ANCS_IN_CP = 3 };
void nxAppOnAncs(uint8_t what, uint8_t status, const uint8_t *data, size_t len);

bool nxBleBegin(const char *name);
void nxBleMac(uint8_t mac[6]); // BT MAC (public identity address)
const char *nxBleStack();      // "Bluedroid" / "NimBLE"

void nxBleSetInfo(const uint8_t *value, size_t len);
// Always open and connectable (paired or not); AUTH (K) decides who is the owner.
void nxBleAdvertise();
void nxBleStopAdvertising();
// Advertising alternates every NX_ADV_ROTATE_MS between two payloads, since
// the service UUID, the ANCS solicitation UUID and the name don't fit in
// 31 + 31 bytes together (the name, 14 bytes, stays in the scan response):
//   A: flags + 128-bit service UUID (what the phone apps scan for)
//   B: flags + ANCS solicitation UUID (iOS: "Share System Notifications")
// Call regularly from the protocol task.
#define NX_ADV_ROTATE_MS 1000u
void nxBleAdvTick(uint32_t nowMs);

bool nxBleConnected();
// Largest notification payload on the current link (MTU - 3, at most 512).
uint16_t nxBleChunk();
// Sends one notification of at most nxBleChunk() bytes on TX. Retries while
// the stack is out of buffers; false when the link or subscription is gone.
// Call from one task only (the protocol task).
bool nxBleNotify(const uint8_t *data, size_t len);
void nxBleDisconnect();

// GATT client on the current connection: look for ANCS, subscribe to Data
// Source then Notification Source; the outcome comes through nxAppOnAncs.
// Call once per connection, after AUTH. Does nothing without a connection.
void nxBleAncsStart();
// Write with response to the ANCS Control Point; the result comes back as
// NX_ANCS_IN_CP. false: not sent (no ANCS on this link, or the stack refused).
bool nxBleAncsWriteCp(const uint8_t *data, size_t len);

int nxBleBondCount();
void nxBleDeleteAllBonds();
// Keep the bond of the connected peer, delete every other one.
void nxBleKeepOnlyCurrentPeer();
// Remember the current bonds at connect time; on a failed AUTH, delete only
// the bonds made since, so a stranger leaves nothing and the owner keeps theirs.
void nxBleSnapshotBonds();
void nxBleDropNewBonds();
