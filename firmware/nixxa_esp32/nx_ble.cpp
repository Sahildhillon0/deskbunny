// nixxa: BLE peripheral. See nx_ble.h.
#include "nx_ble.h"
#include "nx_ancs.h"

#include <Arduino.h>
#include <BLEDevice.h>
#include <BLEServer.h>
#include <BLEUtils.h>
#include <BLESecurity.h>
#include <esp_mac.h>
#include <string.h>

#if defined(CONFIG_BLUEDROID_ENABLED)
#include <BLE2902.h>
#include <esp_gap_ble_api.h>
#include <esp_gattc_api.h>
#elif defined(CONFIG_NIMBLE_ENABLED)
#include <host/ble_hs.h>
#include <host/ble_store.h>
#include <host/ble_gatt.h>
#else
#error "nixxa needs the BLE stack (Bluedroid or NimBLE) enabled in the core"
#endif

static BLEServer *server;
static BLECharacteristic *chInfo, *chRx, *chTx;
static volatile bool connected;
static volatile uint16_t mtu = 23;
static TaskHandle_t txTask;
static volatile int txStatus;
#define TX_PENDING (-1)

// Advertising payloads (see nxBleAdvTick): A = service UUID, B = ANCS
// solicitation; the scan response carries the name.
static uint8_t advA[31], advALen, advB[31], advBLen, rspData[31], rspLen;
static volatile uint8_t advPhase; // 0: A, 1: B
static uint32_t advRotAt;

// ANCS client progress on the current link (both stacks).
enum {
  AN_OFF = 0,  // not started / no ANCS / link gone
  AN_DISC,     // service + characteristics
  AN_DSC_DS,   // NimBLE: Data Source CCCD
  AN_DSC_NS,   // NimBLE: Notification Source CCCD
  AN_SUB_DS,   // CCCD write: Data Source
  AN_SUB_NS,   // CCCD write: Notification Source (READY already sent)
  AN_ON
};
static volatile uint8_t anStep;
static volatile uint16_t anStart, anEnd, hNs, hNsCccd, hCp, hDs, hDsCccd;

static void anReset()
{
  anStep = AN_OFF;
  anStart = 0;
  anEnd = 0;
  hNs = 0;
  hNsCccd = 0;
  hCp = 0;
  hDs = 0;
  hDsCccd = 0;
}

// Discovery failed or ANCS isn't there (an Android phone): off for this link.
static void anNone()
{
  if (anStep == AN_OFF) return; // already reported, or the link is gone
  anStep = AN_OFF;
  nxAppOnAncs(NX_ANCS_IN_READY, 0, NULL, 0);
}

#if defined(CONFIG_BLUEDROID_ENABLED)
static esp_bd_addr_t peerAddr;    // as seen on the connect event
static esp_bd_addr_t peerAuthAddr; // as reported by pairing/encryption
static volatile bool privacyReady, advWanted;
static volatile bool advOwnerOnly;
static volatile bool advRunning;
static volatile int advRotPending; // adv-data-only updates in flight (rotation)
#define NX_GATTC_APP_ID 0x4E
static volatile esp_gatt_if_t gcIf = ESP_GATT_IF_NONE;
static volatile uint16_t gcConn = 0xFFFF;
static esp_bd_addr_t gcBda;
#elif defined(CONFIG_NIMBLE_ENABLED)
static volatile uint16_t connHandle = 0xFFFF;
#endif

// ---------------------------------------------------------------------------
// Callbacks
// ---------------------------------------------------------------------------
class ServerCb : public BLEServerCallbacks {
#if defined(CONFIG_BLUEDROID_ENABLED)
  void onConnect(BLEServer *, esp_ble_gatts_cb_param_t *p) override
  {
    memcpy(peerAddr, p->connect.remote_bda, sizeof(esp_bd_addr_t));
    memset(peerAuthAddr, 0, sizeof(peerAuthAddr));
    mtu = 23;
    advRunning = false; // the controller stops advertising on a connection
    connected = true;
    nxAppOnConnect();
  }
  void onDisconnect(BLEServer *, esp_ble_gatts_cb_param_t *) override
  {
    connected = false;
    nxAppOnDisconnect();
  }
  void onMtuChanged(BLEServer *, esp_ble_gatts_cb_param_t *p) override { mtu = p->mtu.mtu; }
#else
  void onConnect(BLEServer *, ble_gap_conn_desc *d) override
  {
    connHandle = d->conn_handle;
    mtu = 23;
    connected = true;
    nxAppOnConnect();
    // "The device requests an MTU of 517": ATT lets either side start the exchange.
    ble_gattc_exchange_mtu(d->conn_handle, NULL, NULL);
  }
  void onDisconnect(BLEServer *, ble_gap_conn_desc *) override
  {
    connected = false;
    connHandle = 0xFFFF;
    anReset();
    nxAppOnDisconnect();
  }
  void onMtuChanged(BLEServer *, ble_gap_conn_desc *, uint16_t m) override { mtu = m; }
#endif
};

class SecurityCb : public BLESecurityCallbacks {
  uint32_t onPassKeyRequest() override { return 0; }
  void onPassKeyNotify(uint32_t) override {}
  bool onSecurityRequest() override { return true; }
  bool onConfirmPIN(uint32_t) override { return true; } // Just Works: nothing to compare
#if defined(CONFIG_BLUEDROID_ENABLED)
  void onAuthenticationComplete(esp_ble_auth_cmpl_t d) override
  {
    if (d.success) memcpy(peerAuthAddr, d.bd_addr, sizeof(esp_bd_addr_t));
    nxAppOnEncrypted(d.success);
  }
#else
  void onAuthenticationComplete(ble_gap_conn_desc *desc, int status) override
  {
    nxAppOnEncrypted(status == 0 && desc && desc->sec_state.encrypted);
  }
#endif
};

class RxCb : public BLECharacteristicCallbacks {
  void onWrite(BLECharacteristic *c) override
  {
    // getData() is the value the stack just stored; copy it out right away.
    nxAppOnRx(c->getData(), c->getLength());
  }
};

class TxCb : public BLECharacteristicCallbacks {
  void onStatus(BLECharacteristic *, Status s, uint32_t) override
  {
    // NimBLE also reports NOTIFY_TX completions later from the host task;
    // only the synchronous result of our own notify() call counts.
    if (xTaskGetCurrentTaskHandle() == txTask) txStatus = (int)s;
  }
};

// ---------------------------------------------------------------------------
// Bluedroid: raw advertising with an RPA and the filter accept list. The
// library's own advertiser is never created on this backend (it would react
// to the raw-data events and restart advertising with its own parameters).
// ---------------------------------------------------------------------------
#if defined(CONFIG_BLUEDROID_ENABLED)
static void bdAdvConfigure()
{
  // -> RAW_SET_COMPLETE -> scan rsp -> SCAN_RSP_RAW_SET_COMPLETE -> start
  if (advPhase) esp_ble_gap_config_adv_data_raw(advB, advBLen);
  else esp_ble_gap_config_adv_data_raw(advA, advALen);
}

static void bdStartAdvertising()
{
  esp_ble_adv_params_t p = {};
  p.adv_int_min = 0xA0; // 100 ms
  p.adv_int_max = 0x140; // 200 ms
  p.adv_type = ADV_TYPE_IND;
  p.own_addr_type = privacyReady ? BLE_ADDR_TYPE_RPA_PUBLIC : BLE_ADDR_TYPE_PUBLIC;
  p.channel_map = ADV_CHNL_ALL;
  p.adv_filter_policy = advOwnerOnly ? ADV_FILTER_ALLOW_SCAN_WLST_CON_WLST : ADV_FILTER_ALLOW_SCAN_ANY_CON_ANY;
  esp_err_t rc = esp_ble_gap_start_advertising(&p);
  if (rc != ESP_OK) log_e("start_advertising: %d", rc);
}

static void bdGap(esp_gap_ble_cb_event_t ev, esp_ble_gap_cb_param_t *p)
{
  switch (ev) {
    case ESP_GAP_BLE_SET_LOCAL_PRIVACY_COMPLETE_EVT:
      privacyReady = p->local_privacy_cmpl.status == ESP_BT_STATUS_SUCCESS;
      if (advWanted && !connected) { // restart with the resolvable private address
        esp_ble_gap_stop_advertising();
        bdAdvConfigure();
      }
      break;
    case ESP_GAP_BLE_ADV_DATA_RAW_SET_COMPLETE_EVT:
      if (advRotPending > 0) { // rotation: only the payload changed
        advRotPending = advRotPending - 1;
        break;
      }
      esp_ble_gap_config_scan_rsp_data_raw(rspData, rspLen);
      break;
    case ESP_GAP_BLE_SCAN_RSP_DATA_RAW_SET_COMPLETE_EVT:
      if (advWanted && !connected && !advRunning) bdStartAdvertising();
      break;
    case ESP_GAP_BLE_ADV_START_COMPLETE_EVT:
      if (p->adv_start_cmpl.status == ESP_BT_STATUS_SUCCESS) advRunning = true;
      break;
    case ESP_GAP_BLE_ADV_STOP_COMPLETE_EVT:
      advRunning = false;
      break;
    default:
      break;
  }
}

// Identity address of a bond: the one from its ID key when the peer sent one
// (phones do: they use resolvable private addresses), else the bond address.
static void bdIdentity(const esp_ble_bond_dev_t *b, esp_bd_addr_t out, esp_ble_wl_addr_type_t *type)
{
  if (b->bond_key.key_mask & ESP_LE_KEY_PID) {
    memcpy(out, b->bond_key.pid_key.static_addr, sizeof(esp_bd_addr_t));
    *type = b->bond_key.pid_key.addr_type == BLE_ADDR_TYPE_PUBLIC ? BLE_WL_ADDR_TYPE_PUBLIC : BLE_WL_ADDR_TYPE_RANDOM;
  } else {
    memcpy(out, b->bd_addr, sizeof(esp_bd_addr_t));
    *type = b->bd_addr_type == BLE_ADDR_TYPE_PUBLIC ? BLE_WL_ADDR_TYPE_PUBLIC : BLE_WL_ADDR_TYPE_RANDOM;
  }
}

static int bdBonds(esp_ble_bond_dev_t *list, int max)
{
  int n = esp_ble_get_bond_device_num();
  if (n <= 0) return 0;
  if (n > max) n = max;
  if (esp_ble_get_bond_device_list(&n, list) != ESP_OK) return 0;
  return n;
}
#define BD_MAX_BONDS 15 // CONFIG_BT_SMP_MAX_BONDS in the core's sdkconfig

// ---------------------------------------------------------------------------
// Bluedroid ANCS client. A registered GATTC app gets a CONNECT event for every
// link, the ones where we are the peripheral included, so it can run GATT
// client procedures on the phone's connection (as in ESP-IDF's ble_ancs demo).
// Runs in the Bluedroid BTC task.
// ---------------------------------------------------------------------------
static bool bdFindChar(const uint8_t uuid[16], uint16_t *val, uint16_t *cccd)
{
  esp_bt_uuid_t u = {};
  u.len = ESP_UUID_LEN_128;
  memcpy(u.uuid.uuid128, uuid, 16);
  esp_gattc_char_elem_t ch = {};
  uint16_t count = 1;
  if (esp_ble_gattc_get_char_by_uuid(gcIf, gcConn, anStart, anEnd, u, &ch, &count) != ESP_GATT_OK || count == 0)
    return false;
  *val = ch.char_handle;
  if (!cccd) return true;
  esp_bt_uuid_t d = {};
  d.len = ESP_UUID_LEN_16;
  d.uuid.uuid16 = ESP_GATT_UUID_CHAR_CLIENT_CONFIG;
  esp_gattc_descr_elem_t de = {};
  count = 1;
  if (esp_ble_gattc_get_descr_by_char_handle(gcIf, gcConn, ch.char_handle, d, &de, &count) != ESP_GATT_OK || count == 0)
    return false;
  *cccd = de.handle;
  return true;
}

static bool bdSubscribe(uint16_t cccd)
{
  uint8_t on[2] = { 1, 0 }; // notifications
  return esp_ble_gattc_write_char_descr(gcIf, gcConn, cccd, sizeof(on), on, ESP_GATT_WRITE_TYPE_RSP,
                                        ESP_GATT_AUTH_REQ_NONE) == ESP_OK;
}

static void bdGattc(esp_gattc_cb_event_t ev, esp_gatt_if_t ifc, esp_ble_gattc_cb_param_t *p)
{
  if (ev == ESP_GATTC_REG_EVT) {
    if (p->reg.app_id == NX_GATTC_APP_ID && p->reg.status == ESP_GATT_OK) gcIf = ifc;
    return;
  }
  if (gcIf == ESP_GATT_IF_NONE || ifc != gcIf) return;
  switch (ev) {
    case ESP_GATTC_CONNECT_EVT:
      gcConn = p->connect.conn_id;
      memcpy(gcBda, p->connect.remote_bda, sizeof(esp_bd_addr_t));
      anReset();
      break;
    case ESP_GATTC_DISCONNECT_EVT:
      gcConn = 0xFFFF;
      anReset();
      break;
    case ESP_GATTC_SEARCH_RES_EVT:
      if (anStep == AN_DISC && p->search_res.srvc_id.uuid.len == ESP_UUID_LEN_128 &&
          memcmp(p->search_res.srvc_id.uuid.uuid.uuid128, nx_ancs_uuid_svc, 16) == 0) {
        anStart = p->search_res.start_handle;
        anEnd = p->search_res.end_handle;
      }
      break;
    case ESP_GATTC_SEARCH_CMPL_EVT: {
      if (anStep != AN_DISC) break;
      uint16_t ns = 0, nsc = 0, cp = 0, ds = 0, dsc = 0;
      if (p->search_cmpl.status != ESP_GATT_OK || anStart == 0 || !bdFindChar(nx_ancs_uuid_ns, &ns, &nsc) ||
          !bdFindChar(nx_ancs_uuid_cp, &cp, NULL) || !bdFindChar(nx_ancs_uuid_ds, &ds, &dsc)) {
        anNone();
        break;
      }
      hNs = ns;
      hNsCccd = nsc;
      hCp = cp;
      hDs = ds;
      hDsCccd = dsc;
      // Local routing of the notifications to this app (no air traffic).
      esp_ble_gattc_register_for_notify(gcIf, gcBda, ds);
      esp_ble_gattc_register_for_notify(gcIf, gcBda, ns);
      anStep = AN_SUB_DS; // Data Source first, so no answer can be missed
      if (!bdSubscribe(dsc)) anNone();
      break;
    }
    case ESP_GATTC_WRITE_DESCR_EVT:
      if (anStep == AN_SUB_DS) {
        if (p->write.status != ESP_GATT_OK) {
          log_w("ANCS: Data Source subscribe failed: 0x%02x", p->write.status);
          anNone();
          break;
        }
        anStep = AN_SUB_NS;
        nxAppOnAncs(NX_ANCS_IN_READY, 1, NULL, 0);
        if (!bdSubscribe(hNsCccd)) log_w("ANCS: Notification Source subscribe not sent");
      } else if (anStep == AN_SUB_NS) {
        if (p->write.status != ESP_GATT_OK) log_w("ANCS: Notification Source subscribe failed: 0x%02x", p->write.status);
        anStep = AN_ON;
      }
      break;
    case ESP_GATTC_WRITE_CHAR_EVT:
      if (anStep >= AN_SUB_NS && p->write.handle == hCp) nxAppOnAncs(NX_ANCS_IN_CP, (uint8_t)p->write.status, NULL, 0);
      break;
    case ESP_GATTC_NOTIFY_EVT:
      if (anStep < AN_SUB_NS || p->notify.conn_id != gcConn) break;
      if (p->notify.handle == hDs) nxAppOnAncs(NX_ANCS_IN_DS, 0, p->notify.value, p->notify.value_len);
      else if (p->notify.handle == hNs) nxAppOnAncs(NX_ANCS_IN_NS, 0, p->notify.value, p->notify.value_len);
      break;
    default:
      break;
  }
}
#endif

// ---------------------------------------------------------------------------
// NimBLE ANCS client: GATT client procedures on the phone's connection, run
// in the NimBLE host task. Notifications reach us through a GAP listener.
// ---------------------------------------------------------------------------
#if defined(CONFIG_NIMBLE_ENABLED)
static ble_uuid128_t nbUuid(const uint8_t le[16])
{
  ble_uuid128_t u;
  u.u.type = BLE_UUID_TYPE_128;
  memcpy(u.value, le, 16);
  return u;
}

#define NB_CHR_MAX 12
static uint16_t nbChrDef[NB_CHR_MAX];
static uint8_t nbChrN;

// Last handle of the characteristic whose value handle is val.
static uint16_t nbChrEnd(uint16_t val)
{
  uint16_t end = anEnd;
  for (uint8_t i = 0; i < nbChrN; i++)
    if (nbChrDef[i] > val && nbChrDef[i] - 1 < end) end = (uint16_t)(nbChrDef[i] - 1);
  return end;
}

static int nbSubCb(uint16_t conn, const struct ble_gatt_error *err, struct ble_gatt_attr *, void *);
static int nbDscCb(uint16_t conn, const struct ble_gatt_error *err, uint16_t chrVal, const struct ble_gatt_dsc *dsc, void *);

static bool nbSubscribe(uint16_t cccd)
{
  static const uint8_t on[2] = { 1, 0 };
  return ble_gattc_write_flat(connHandle, cccd, on, sizeof(on), nbSubCb, NULL) == 0;
}

static int nbSubCb(uint16_t conn, const struct ble_gatt_error *err, struct ble_gatt_attr *, void *)
{
  if (conn != connHandle) return 0;
  if (anStep == AN_SUB_DS) {
    if (err->status != 0) {
      log_w("ANCS: Data Source subscribe failed: %d", err->status);
      anNone();
      return 0;
    }
    anStep = AN_SUB_NS;
    nxAppOnAncs(NX_ANCS_IN_READY, 1, NULL, 0);
    if (!nbSubscribe(hNsCccd)) log_w("ANCS: Notification Source subscribe not sent");
  } else if (anStep == AN_SUB_NS) {
    if (err->status != 0) log_w("ANCS: Notification Source subscribe failed: %d", err->status);
    anStep = AN_ON;
  }
  return 0;
}

static int nbDscCb(uint16_t conn, const struct ble_gatt_error *err, uint16_t chrVal, const struct ble_gatt_dsc *dsc, void *)
{
  if (conn != connHandle || (anStep != AN_DSC_DS && anStep != AN_DSC_NS)) return 0;
  if (err->status == 0 && dsc) {
    ble_uuid16_t cccd;
    cccd.u.type = BLE_UUID_TYPE_16;
    cccd.value = BLE_GATT_DSC_CLT_CFG_UUID16;
    if (ble_uuid_cmp(&dsc->uuid.u, &cccd.u) == 0) {
      if (chrVal == hDs) hDsCccd = dsc->handle;
      else if (chrVal == hNs) hNsCccd = dsc->handle;
    }
    return 0;
  }
  if (err->status != BLE_HS_EDONE) {
    anNone();
    return 0;
  }
  if (anStep == AN_DSC_DS) {
    anStep = AN_DSC_NS;
    if (!hDsCccd || ble_gattc_disc_all_dscs(conn, hNs, nbChrEnd(hNs), nbDscCb, NULL) != 0) anNone();
  } else {
    anStep = AN_SUB_DS; // Data Source first, so no answer can be missed
    if (!hNsCccd || !nbSubscribe(hDsCccd)) anNone();
  }
  return 0;
}

static int nbChrCb(uint16_t conn, const struct ble_gatt_error *err, const struct ble_gatt_chr *chr, void *)
{
  if (conn != connHandle || anStep != AN_DISC) return 0;
  if (err->status == 0 && chr) {
    if (nbChrN < NB_CHR_MAX) nbChrDef[nbChrN++] = chr->def_handle;
    ble_uuid128_t ns = nbUuid(nx_ancs_uuid_ns), cp = nbUuid(nx_ancs_uuid_cp), ds = nbUuid(nx_ancs_uuid_ds);
    if (ble_uuid_cmp(&chr->uuid.u, &ns.u) == 0) hNs = chr->val_handle;
    else if (ble_uuid_cmp(&chr->uuid.u, &cp.u) == 0) hCp = chr->val_handle;
    else if (ble_uuid_cmp(&chr->uuid.u, &ds.u) == 0) hDs = chr->val_handle;
    return 0;
  }
  if (err->status != BLE_HS_EDONE || !hNs || !hCp || !hDs) {
    anNone();
    return 0;
  }
  anStep = AN_DSC_DS;
  if (ble_gattc_disc_all_dscs(conn, hDs, nbChrEnd(hDs), nbDscCb, NULL) != 0) anNone();
  return 0;
}

static int nbSvcCb(uint16_t conn, const struct ble_gatt_error *err, const struct ble_gatt_svc *svc, void *)
{
  if (conn != connHandle || anStep != AN_DISC) return 0;
  if (err->status == 0 && svc) {
    anStart = svc->start_handle;
    anEnd = svc->end_handle;
    return 0;
  }
  // BLE_HS_EATT with "attribute not found" = no ANCS (Android).
  if (err->status != BLE_HS_EDONE || anStart == 0) {
    anNone();
    return 0;
  }
  nbChrN = 0;
  if (ble_gattc_disc_all_chrs(conn, anStart, anEnd, nbChrCb, NULL) != 0) anNone();
  return 0;
}

static int nbCpCb(uint16_t conn, const struct ble_gatt_error *err, struct ble_gatt_attr *, void *)
{
  if (conn != connHandle || anStep < AN_SUB_NS) return 0;
  int st = err->status;
  uint8_t att = 0;
  if (st >= BLE_HS_ERR_ATT_BASE && st < BLE_HS_ERR_ATT_BASE + 0x100) att = (uint8_t)(st - BLE_HS_ERR_ATT_BASE);
  else if (st != 0) att = 0xFF; // host error (link gone, timeout)
  nxAppOnAncs(NX_ANCS_IN_CP, att, NULL, 0);
  return 0;
}

static struct ble_gap_event_listener nbListener;
static int nbGapEvent(struct ble_gap_event *e, void *)
{
  if (e->type != BLE_GAP_EVENT_NOTIFY_RX || anStep < AN_SUB_NS || e->notify_rx.conn_handle != connHandle) return 0;
  uint16_t h = e->notify_rx.attr_handle;
  if (h != hDs && h != hNs) return 0;
  static uint8_t buf[512]; // host task only
  uint16_t n = 0;
  ble_hs_mbuf_to_flat(e->notify_rx.om, buf, sizeof(buf), &n); // copies what fits
  nxAppOnAncs(h == hDs ? NX_ANCS_IN_DS : NX_ANCS_IN_NS, 0, buf, n);
  return 0;
}
#endif

// ---------------------------------------------------------------------------
bool nxBleBegin(const char *name)
{
  if (!BLEDevice::init(name)) return false;
  BLEDevice::setMTU(517);

  BLESecurity *sec = new BLESecurity();
  sec->setCapability(ESP_IO_CAP_NONE);           // Just Works
  sec->setInitEncryptionKey(ESP_BLE_ENC_KEY_MASK | ESP_BLE_ID_KEY_MASK);
  sec->setRespEncryptionKey(ESP_BLE_ENC_KEY_MASK | ESP_BLE_ID_KEY_MASK); // get the phone's IRK
  sec->setAuthenticationMode(true, false, true); // bonding, no MITM, LE Secure Connections
  sec->setForceAuthentication(true);             // ask for encryption as soon as a phone connects
  BLEDevice::setSecurityCallbacks(new SecurityCb());
#if defined(CONFIG_BLUEDROID_ENABLED)
  uint8_t only = ESP_BLE_ONLY_ACCEPT_SPECIFIED_AUTH_ENABLE; // refuse legacy pairing
  esp_ble_gap_set_security_param(ESP_BLE_SM_ONLY_ACCEPT_SPECIFIED_SEC_AUTH, &only, 1);
  BLEDevice::setCustomGapHandler(bdGap);
  BLEDevice::setCustomGattcHandler(bdGattc);
  if (esp_ble_gattc_app_register(NX_GATTC_APP_ID) != ESP_OK) log_e("ANCS: GATTC app register failed");
  // Local privacy turns on controller address resolution, so the accept list
  // (identity addresses) matches the owner's resolvable private address.
  esp_ble_gap_config_local_privacy(true);
#else
  ble_hs_cfg.sm_sc_only = 1; // refuse legacy pairing
  if (ble_gap_event_listener_register(&nbListener, nbGapEvent, NULL) != 0) log_e("ANCS: GAP listener failed");
  // Bonded IRKs are loaded into the controller's resolving list by NimBLE itself.
#endif

  server = BLEDevice::createServer();
  server->setCallbacks(new ServerCb());
  server->advertiseOnDisconnect(false);
  BLEService *svc = server->createService(BLEUUID(NX_SVC_UUID), 16);

  chInfo = svc->createCharacteristic(NX_INFO_UUID, BLECharacteristic::PROPERTY_READ | BLECharacteristic::PROPERTY_READ_ENC);
  chRx = svc->createCharacteristic(NX_RX_UUID, BLECharacteristic::PROPERTY_WRITE | BLECharacteristic::PROPERTY_WRITE_NR |
                                                   BLECharacteristic::PROPERTY_WRITE_ENC);
  chTx = svc->createCharacteristic(NX_TX_UUID, BLECharacteristic::PROPERTY_NOTIFY);
#if defined(CONFIG_BLUEDROID_ENABLED)
  // Bluedroid has no *_ENC properties: encryption is an attribute permission.
  chInfo->setAccessPermissions(ESP_GATT_PERM_READ_ENCRYPTED);
  chRx->setAccessPermissions(ESP_GATT_PERM_WRITE_ENCRYPTED);
  chTx->setAccessPermissions(ESP_GATT_PERM_READ_ENCRYPTED);
  BLE2902 *cccd = new BLE2902();
  cccd->setAccessPermissions(ESP_GATT_PERM_READ_ENCRYPTED | ESP_GATT_PERM_WRITE_ENCRYPTED);
  chTx->addDescriptor(cccd);
#endif
  chRx->setCallbacks(new RxCb());
  chTx->setCallbacks(new TxCb());
  svc->start();

  // Advertising payloads: A = flags + the 128-bit service UUID, B = flags +
  // the ANCS solicitation UUID; scan response = the complete name. 18 + 18 +
  // 14 bytes of AD structures can't share two 31-byte packets (any packet
  // with two of them needs >= 32), so A and B alternate (nxBleAdvTick).
  BLEUUID u(NX_SVC_UUID);
#if defined(CONFIG_BLUEDROID_ENABLED)
  const uint8_t *le = u.getNative()->uuid.uuid128; // little-endian, as on air
#else
  const uint8_t *le = u.getNative()->u128.value;   // little-endian, as on air
#endif
  advALen = (uint8_t)nx_adv_flags_uuid(advA, NX_AD_SVC128, le);
  advBLen = (uint8_t)nx_adv_flags_uuid(advB, NX_AD_SOLICIT128, nx_ancs_uuid_svc);
  size_t nl = strlen(name);
  if (nl > 29) nl = 29;
  rspData[0] = (uint8_t)(nl + 1);
  rspData[1] = 0x09;
  memcpy(rspData + 2, name, nl);
  rspLen = (uint8_t)(nl + 2);
#if defined(CONFIG_NIMBLE_ENABLED)
  BLEAdvertising *adv = BLEDevice::getAdvertising();
  adv->setScanResponse(true);
  adv->setMinInterval(0xA0);
  adv->setMaxInterval(0x140);
#endif
  return true;
}

const char *nxBleStack()
{
#if defined(CONFIG_BLUEDROID_ENABLED)
  return "Bluedroid";
#else
  return "NimBLE";
#endif
}

void nxBleMac(uint8_t mac[6])
{
  if (esp_read_mac(mac, ESP_MAC_BT) != ESP_OK) memset(mac, 0, 6);
}

void nxBleSetInfo(const uint8_t *value, size_t len) { chInfo->setValue(value, len); }

void nxBleStopAdvertising()
{
#if defined(CONFIG_BLUEDROID_ENABLED)
  advWanted = false;
  esp_ble_gap_stop_advertising();
#else
  BLEDevice::getAdvertising()->stop();
#endif
}

void nxBleAdvertise()
{
  if (connected) return;
  // Always open and connectable, paired or not. A paired device used to
  // advertise only to the owner's accept-list entry, but after a reboot the
  // controller did not match the phone's resolvable private address against
  // it, so the owner could not reconnect. Ownership is enforced by AUTH (K),
  // and a peer that fails it loses the bond it just made (nxBleDropNewBonds).
#if defined(CONFIG_BLUEDROID_ENABLED)
  esp_ble_gap_stop_advertising();
  esp_ble_gap_clear_whitelist();
  advOwnerOnly = false;
  advWanted = true;
  bdAdvConfigure(); // if privacy is still being set up, its completion event restarts this
#else
  BLEAdvertising *adv = BLEDevice::getAdvertising();
  adv->stop();
  ble_gap_wl_set(NULL, 0);
  adv->setScanFilter(false, false);
  BLEAdvertisementData ad, rsp;
  if (advPhase) ad.addData((char *)advB, advBLen);
  else ad.addData((char *)advA, advALen);
  rsp.addData((char *)rspData, rspLen);
  adv->setAdvertisementData(ad); // custom data: the library adds nothing of its own
  adv->setScanResponseData(rsp);
  BLEDevice::startAdvertising();
#endif
}

void nxBleAdvTick(uint32_t nowMs)
{
  if (connected || (uint32_t)(nowMs - advRotAt) < NX_ADV_ROTATE_MS) return;
  advRotAt = nowMs;
#if defined(CONFIG_BLUEDROID_ENABLED)
  if (!advWanted || !advRunning) return;
  advPhase ^= 1;
  advRotPending = advRotPending + 1;
  if (advPhase) esp_ble_gap_config_adv_data_raw(advB, advBLen);
  else esp_ble_gap_config_adv_data_raw(advA, advALen);
#else
  if (!ble_gap_adv_active()) return;
  advPhase ^= 1;
  int rc = advPhase ? ble_gap_adv_set_data(advB, advBLen) : ble_gap_adv_set_data(advA, advALen);
  if (rc != 0) log_w("adv rotate: %d", rc);
#endif
}

bool nxBleConnected() { return connected; }

uint16_t nxBleChunk()
{
  uint16_t m = mtu;
  if (m < 23) m = 23;
  m = (uint16_t)(m - 3);
  return m > 512 ? 512 : m;
}

bool nxBleNotify(const uint8_t *data, size_t len)
{
  txTask = xTaskGetCurrentTaskHandle();
  for (int attempt = 0; attempt < 400; attempt++) { // ~2 s of retries
    if (!connected) return false;
    txStatus = TX_PENDING;
    chTx->setValue(data, len);
    chTx->notify();
    int s = txStatus;
    if (s == TX_PENDING || s == BLECharacteristicCallbacks::SUCCESS_NOTIFY) {
#if defined(CONFIG_BLUEDROID_ENABLED)
      // Bluedroid queues notifications without back-pressure; pace them.
      vTaskDelay(pdMS_TO_TICKS(4));
#endif
      return true;
    }
    if (s != BLECharacteristicCallbacks::ERROR_GATT) return false; // not subscribed / no client
    vTaskDelay(pdMS_TO_TICKS(5)); // out of buffers: wait for the stack to drain
  }
  return false;
}

void nxBleDisconnect()
{
  if (!connected) return;
#if defined(CONFIG_BLUEDROID_ENABLED)
  esp_ble_gap_disconnect(peerAddr);
#else
  if (connHandle != 0xFFFF) ble_gap_terminate(connHandle, BLE_ERR_REM_USER_CONN_TERM);
#endif
}

void nxBleAncsStart()
{
  if (!connected) return;
  anReset();
  anStep = AN_DISC;
#if defined(CONFIG_BLUEDROID_ENABLED)
  if (gcIf == ESP_GATT_IF_NONE || gcConn == 0xFFFF) {
    anNone();
    return;
  }
  esp_bt_uuid_t u = {};
  u.len = ESP_UUID_LEN_128;
  memcpy(u.uuid.uuid128, nx_ancs_uuid_svc, 16);
  if (esp_ble_gattc_search_service(gcIf, gcConn, &u) != ESP_OK) anNone();
#else
  static ble_uuid128_t svc;
  svc = nbUuid(nx_ancs_uuid_svc);
  if (connHandle == 0xFFFF || ble_gattc_disc_svc_by_uuid(connHandle, &svc.u, nbSvcCb, NULL) != 0) anNone();
#endif
}

bool nxBleAncsWriteCp(const uint8_t *data, size_t len)
{
  if (!connected || anStep < AN_SUB_NS || !hCp) return false;
#if defined(CONFIG_BLUEDROID_ENABLED)
  return esp_ble_gattc_write_char(gcIf, gcConn, hCp, (uint16_t)len, (uint8_t *)data, ESP_GATT_WRITE_TYPE_RSP,
                                  ESP_GATT_AUTH_REQ_NONE) == ESP_OK;
#else
  return ble_gattc_write_flat(connHandle, hCp, data, (uint16_t)len, nbCpCb, NULL) == 0;
#endif
}

int nxBleBondCount()
{
#if defined(CONFIG_BLUEDROID_ENABLED)
  int n = esp_ble_get_bond_device_num();
  return n < 0 ? 0 : n;
#else
  ble_addr_t peers[MYNEWT_VAL(BLE_STORE_MAX_BONDS)];
  int n = 0;
  if (ble_store_util_bonded_peers(peers, &n, MYNEWT_VAL(BLE_STORE_MAX_BONDS)) != 0) return 0;
  return n;
#endif
}

void nxBleDeleteAllBonds()
{
#if defined(CONFIG_BLUEDROID_ENABLED)
  static esp_ble_bond_dev_t list[BD_MAX_BONDS];
  int n = bdBonds(list, BD_MAX_BONDS);
  for (int i = 0; i < n; i++) esp_ble_remove_bond_device(list[i].bd_addr);
  esp_ble_gap_clear_whitelist();
#else
  ble_addr_t peers[MYNEWT_VAL(BLE_STORE_MAX_BONDS)];
  int n = 0;
  if (ble_store_util_bonded_peers(peers, &n, MYNEWT_VAL(BLE_STORE_MAX_BONDS)) == 0)
    for (int i = 0; i < n; i++) ble_store_util_delete_peer(&peers[i]);
  ble_gap_wl_set(NULL, 0);
#endif
}

void nxBleKeepOnlyCurrentPeer()
{
#if defined(CONFIG_BLUEDROID_ENABLED)
  static esp_ble_bond_dev_t list[BD_MAX_BONDS];
  int n = bdBonds(list, BD_MAX_BONDS);
  if (n <= 1) return;
  bool match[BD_MAX_BONDS] = {};
  int matches = 0;
  for (int i = 0; i < n; i++) {
    esp_bd_addr_t id;
    esp_ble_wl_addr_type_t t;
    bdIdentity(&list[i], id, &t);
    const uint8_t *cands[2] = { peerAddr, peerAuthAddr };
    for (int c = 0; c < 2 && !match[i]; c++) {
      if (memcmp(list[i].bd_addr, cands[c], 6) == 0 || memcmp(id, cands[c], 6) == 0) match[i] = true;
    }
    if (match[i]) matches++;
  }
  if (matches == 0) return; // can't tell which bond is ours: keep them all
  for (int i = 0; i < n; i++)
    if (!match[i]) esp_ble_remove_bond_device(list[i].bd_addr);
#else
  struct ble_gap_conn_desc d;
  if (connHandle == 0xFFFF || ble_gap_conn_find(connHandle, &d) != 0) return;
  ble_addr_t peers[MYNEWT_VAL(BLE_STORE_MAX_BONDS)];
  int n = 0;
  if (ble_store_util_bonded_peers(peers, &n, MYNEWT_VAL(BLE_STORE_MAX_BONDS)) != 0) return;
  for (int i = 0; i < n; i++)
    if (ble_addr_cmp(&peers[i], &d.peer_id_addr) != 0) ble_store_util_delete_peer(&peers[i]);
#endif
}

// Bonds that existed when the current connection started, so a peer that
// fails AUTH can lose the bond it created without touching the owner's.
#if defined(CONFIG_BLUEDROID_ENABLED)
static esp_bd_addr_t snapBonds[BD_MAX_BONDS];
#else
static ble_addr_t snapBonds[MYNEWT_VAL(BLE_STORE_MAX_BONDS)];
#endif
static int snapN;

void nxBleSnapshotBonds()
{
  snapN = 0;
#if defined(CONFIG_BLUEDROID_ENABLED)
  static esp_ble_bond_dev_t list[BD_MAX_BONDS];
  int n = bdBonds(list, BD_MAX_BONDS);
  for (int i = 0; i < n; i++) memcpy(snapBonds[snapN++], list[i].bd_addr, sizeof(esp_bd_addr_t));
#else
  int n = 0;
  if (ble_store_util_bonded_peers(snapBonds, &n, MYNEWT_VAL(BLE_STORE_MAX_BONDS)) == 0) snapN = n;
#endif
}

void nxBleDropNewBonds()
{
#if defined(CONFIG_BLUEDROID_ENABLED)
  static esp_ble_bond_dev_t list[BD_MAX_BONDS];
  int n = bdBonds(list, BD_MAX_BONDS);
  for (int i = 0; i < n; i++) {
    bool old = false;
    for (int j = 0; j < snapN && !old; j++) old = memcmp(list[i].bd_addr, snapBonds[j], sizeof(esp_bd_addr_t)) == 0;
    if (!old) esp_ble_remove_bond_device(list[i].bd_addr);
  }
#else
  ble_addr_t peers[MYNEWT_VAL(BLE_STORE_MAX_BONDS)];
  int n = 0;
  if (ble_store_util_bonded_peers(peers, &n, MYNEWT_VAL(BLE_STORE_MAX_BONDS)) != 0) return;
  for (int i = 0; i < n; i++) {
    bool old = false;
    for (int j = 0; j < snapN && !old; j++) old = ble_addr_cmp(&peers[i], &snapBonds[j]) == 0;
    if (!old) ble_store_util_delete_peer(&peers[i]);
  }
#endif
}
