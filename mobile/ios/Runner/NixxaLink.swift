// The iPhone <-> bunny link: pairing, reconnect, message queue, status.
// iOS counterpart of mobile/android/.../link/Link.kt. Everything runs on the
// main queue (the CBCentralManager delegate queue, Flutter's platform thread
// and the timers), so no locking is needed.
// Contract: docs/ARCHITECTURE.md "iOS (phase 3)".
import CoreBluetooth
import Foundation
import Intents
import Security
import UIKit

// MARK: - Persistence

/// Keychain: the bunny's key K, its device ID and the CBPeripheral identifier.
struct Pairing: Codable, Equatable {
  let peripheralId: String
  let deviceId: String
  let keyHex: String

  var key: Data? { PairingQr.unhex(keyHex) }
}

enum PairingStore {
  private static let service = "com.zenithratech.nixxa.link"
  private static let account = "pairing"

  private static var base: [String: Any] {
    [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: account,
    ]
  }

  static func load() -> Pairing? {
    var q = base
    q[kSecReturnData as String] = true
    q[kSecMatchLimit as String] = kSecMatchLimitOne
    var out: CFTypeRef?
    guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let d = out as? Data else { return nil }
    guard let p = try? JSONDecoder().decode(Pairing.self, from: d), p.key?.count == 16 else { return nil }
    return p
  }

  @discardableResult
  static func save(_ p: Pairing) -> Bool {
    guard let d = try? JSONEncoder().encode(p) else { return false }
    SecItemDelete(base as CFDictionary)
    var q = base
    q[kSecValueData as String] = d
    q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
    return SecItemAdd(q as CFDictionary, nil) == errSecSuccess
  }

  static func clear() {
    SecItemDelete(base as CFDictionary)
  }
}

/// UserDefaults: the notification config as JSON.
enum ConfigStore {
  private static let key = "nixxa.notifConfig"

  static func load() -> NotifConfig {
    guard let d = UserDefaults.standard.data(forKey: key),
          let c = try? JSONDecoder().decode(NotifConfig.self, from: d) else { return NotifConfig() }
    return c
  }

  static func save(_ c: NotifConfig) {
    if let d = try? JSONEncoder().encode(c) { UserDefaults.standard.set(d, forKey: key) }
  }
}

// MARK: - Link

final class NixxaLink: NSObject {
  static let shared = NixxaLink()

  enum State: String {
    case bluetoothOff, unpaired, disconnected, connecting, connected
  }

  struct Status: Equatable {
    let state: State
    let deviceId: String?
    let fwVersion: String?
    let chip: String?

    func toMap() -> [String: Any] {
      func opt(_ s: String?) -> Any {
        if let s = s { return s }
        return NSNull()
      }
      return ["state": state.rawValue, "deviceId": opt(deviceId), "fwVersion": opt(fwVersion), "chip": opt(chip)]
    }
  }

  private static let restoreId = "com.zenithratech.nixxa.central"
  private static let serviceId = CBUUID(string: Proto.serviceUUID)
  private static let infoId = CBUUID(string: Proto.infoUUID)
  private static let rxId = CBUUID(string: Proto.rxUUID)
  private static let txId = CBUUID(string: Proto.txUUID)
  private static let ownAppId = "com.zenithratech.nixxa"

  private(set) var status = Status(state: .unpaired, deviceId: nil, fwVersion: nil, chip: nil)
  private var listeners = [Int: (Status) -> Void]()
  private var nextListener = 0

  private(set) var config = NotifConfig()
  private var pairing: Pairing?
  private var launched = false

  private var central: CBCentralManager?
  private var stateWaiters = [(CBManagerState) -> Void]()

  // One connection at a time.
  private var device: CBPeripheral?
  private var infoChar: CBCharacteristic?
  private var rxChar: CBCharacteristic?
  private var txChar: CBCharacteristic?
  private var queue: MessageQueue?
  private let reasm = Reassembler()
  private var ready = false
  private var fw: String?
  private var chip: String?
  private var lastDnd: Bool?
  /// False once this connection's firmware refused the trailing ext byte.
  private var configExt = true
  private var writeChunks = [Data]()
  private var writeDone: ((Bool) -> Void)?

  private var setup: Setup?
  private var pairFlow: PairFlow?
  private var reconnectScan = false
  private var reconnectWork: DispatchWorkItem?
  private var backoffMs = 1000
  private var dndTimer: Timer?

  fileprivate enum Step { case discover, notify, info, messages }

  /// One run of: discovery, notifications, INFO, AUTH, SET_TIME, SET_NOTIF_CONFIG, SET_DND.
  fileprivate final class Setup {
    let key: Data
    let expectedId: String
    let isPairing: Bool
    let done: (Result<Void, LinkError>) -> Void
    var step = Step.discover
    var timer: DispatchWorkItem?
    init(key: Data, expectedId: String, isPairing: Bool, done: @escaping (Result<Void, LinkError>) -> Void) {
      self.key = key
      self.expectedId = expectedId
      self.isPairing = isPairing
      self.done = done
    }
  }

  fileprivate final class PairFlow {
    let qr: PairingQr
    let cb: (Result<Status, LinkError>) -> Void
    var scanning = false
    var timer: DispatchWorkItem?
    init(qr: PairingQr, cb: @escaping (Result<Status, LinkError>) -> Void) {
      self.qr = qr
      self.cb = cb
    }
  }

  // MARK: Lifecycle

  /// From AppDelegate.didFinishLaunching. The central is only created when a
  /// bunny is paired (or iOS relaunched us for Bluetooth), so a fresh install
  /// doesn't ask for Bluetooth before the user starts pairing.
  func launch(options: [UIApplication.LaunchOptionsKey: Any]?) {
    if launched { return }
    launched = true
    config = ConfigStore.load()
    pairing = PairingStore.load()
    if pairing != nil || options?[.bluetoothCentrals] != nil { makeCentral() }
    let nc = NotificationCenter.default
    nc.addObserver(self, selector: #selector(appDidBecomeActive), name: UIApplication.didBecomeActiveNotification, object: nil)
    nc.addObserver(self, selector: #selector(appWillResignActive), name: UIApplication.willResignActiveNotification, object: nil)
    nc.addObserver(self, selector: #selector(clockChanged), name: UIApplication.significantTimeChangeNotification, object: nil)
    nc.addObserver(self, selector: #selector(clockChanged), name: NSNotification.Name.NSSystemTimeZoneDidChange, object: nil)
    publish()
  }

  private func makeCentral() {
    if central != nil { return }
    central = CBCentralManager(delegate: self, queue: nil, options: [
      CBCentralManagerOptionRestoreIdentifierKey: NixxaLink.restoreId,
      CBCentralManagerOptionShowPowerAlertKey: false,
    ])
  }

  @objc private func appDidBecomeActive() {
    dndCheck()
    dndTimer?.invalidate()
    // Focus has no change notification; poll while the app is in front.
    dndTimer = Timer.scheduledTimer(withTimeInterval: 15, repeats: true) { [weak self] _ in self?.dndCheck() }
  }

  @objc private func appWillResignActive() {
    dndTimer?.invalidate()
    dndTimer = nil
  }

  @objc private func clockChanged() {
    DispatchQueue.main.async { [weak self] in
      guard let self = self, self.ready else { return }
      self.fire(Proto.opSetTime, Proto.setTimeNow())
    }
  }

  // MARK: Status

  func addListener(_ l: @escaping (Status) -> Void) -> Int {
    nextListener += 1
    listeners[nextListener] = l
    return nextListener
  }

  func removeListener(_ id: Int) { listeners[id] = nil }

  private func computeState() -> State {
    if let c = central {
      switch c.state {
      case .poweredOff, .unauthorized, .unsupported: return .bluetoothOff
      default: break
      }
    }
    if pairFlow != nil { return .connecting }
    guard pairing != nil else { return .unpaired }
    if ready { return .connected }
    if setup != nil || device?.state == .connected { return .connecting }
    return .disconnected
  }

  private func publish() {
    let state = computeState()
    let up = state == .connected
    let st = Status(state: state, deviceId: pairing?.deviceId, fwVersion: up ? fw : nil, chip: up ? chip : nil)
    if st == status { return }
    status = st
    NSLog("nixxa.link status %@", st.state.rawValue)
    for l in listeners.values { l(st) }
  }

  // MARK: Public API (main thread; callbacks on the main thread)

  func pair(_ qrText: String, _ cb: @escaping (Result<Status, LinkError>) -> Void) {
    guard let qr = PairingQr.parse(qrText) else {
      cb(.failure(LinkError(code: "BAD_QR", message: "This is not a nixxa QR code")))
      return
    }
    if let old = pairFlow { endPair(old, .failure(.notConnected("Pairing cancelled"))) }
    let flow = PairFlow(qr: qr, cb: cb)
    pairFlow = flow
    cancelReconnect()
    teardown(cancel: true)
    device = nil // a late disconnect callback for it is then ignored
    publish()
    makeCentral()
    whenStateKnown { [weak self] st in
      guard let self = self, self.pairFlow === flow else { return }
      switch st {
      case .poweredOn:
        self.startPairScan(flow)
      case .unauthorized:
        self.failPair(flow, LinkError(code: "PERMISSION", message: "Bluetooth permission is needed to pair. Allow it in Settings > nixxa."))
      default:
        self.failPair(flow, LinkError(code: "BT_OFF", message: "Bluetooth is off"))
      }
    }
  }

  func forget(_ cb: @escaping () -> Void) {
    if let f = pairFlow { failPair(f, .notConnected("Pairing cancelled")) }
    var finished = false
    let finish: () -> Void = { [weak self] in
      if finished { return }
      finished = true
      self?.forgetLocal()
      cb()
    }
    if ready, let q = queue {
      q.enqueue(Proto.opUnpair, Data()) { _ in finish() }
      after(3000, finish)
    } else {
      finish()
    }
  }

  func request(_ op: UInt8, _ payload: Data, _ cb: @escaping (Result<Data, LinkError>) -> Void) {
    guard ready, let q = queue else {
      cb(.failure(.notConnected()))
      return
    }
    q.enqueue(op, payload, cb)
  }

  func setNotifConfig(_ c: NotifConfig) {
    config = c
    ConfigStore.save(c)
    if ready {
      fire(Proto.opSetNotifConfig, Proto.notifConfig(c, ext: configExt))
      lastDnd = nil
      dndCheck()
    }
    if c.followDnd { requestFocusAuthorization() }
  }

  /// ANCS authorization the user gave the bunny ("Share System Notifications").
  var hasNotificationAccess: Bool { device?.ancsAuthorized ?? false }

  /// LIST_SEEN_APPS (when connected) merged with the built-in catalogue.
  func listApps(_ cb: @escaping ([(appId: String, label: String)]) -> Void) {
    guard ready, let q = queue else {
      cb(AppCatalogue.merge(seen: []))
      return
    }
    q.enqueue(Proto.opListSeenApps, Data()) { r in
      var seen = [SeenApp]()
      if case .success(let d) = r, let s = Proto.parseSeenApps(d) { seen = s }
      cb(AppCatalogue.merge(seen: seen))
    }
  }

  func sendTestNotification(_ cb: @escaping (Result<Data, LinkError>) -> Void) {
    let payload = Proto.notify(
      kind: Proto.kindMessage, notifId: 0x6E69_7861, mood: 0xFF, appId: NixxaLink.ownAppId, appName: "nixxa",
      title: "Hello from nixxa", body: "Notifications reach your bunny. This is what a card looks like.", icon: nil
    )
    request(Proto.opNotify, payload, cb)
  }

  // MARK: Internals

  @discardableResult
  private func after(_ ms: Int, _ f: @escaping () -> Void) -> DispatchWorkItem {
    let w = DispatchWorkItem(block: f)
    DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(ms), execute: w)
    return w
  }

  private func whenStateKnown(_ f: @escaping (CBManagerState) -> Void) {
    guard let c = central else { return }
    if c.state != .unknown && c.state != .resetting {
      f(c.state)
      return
    }
    // The permission prompt keeps the state unknown until the user answers.
    var done = false
    let once: (CBManagerState) -> Void = { st in
      if done { return }
      done = true
      f(st)
    }
    stateWaiters.append(once)
    after(60_000) { [weak self] in once(self?.central?.state ?? .unknown) }
  }

  /// Fire and forget (runtime config, DND, time); dropped while not ready.
  private func fire(_ op: UInt8, _ payload: Data) {
    guard ready, let q = queue else { return }
    q.enqueue(op, payload) { r in
      if case .failure(let e) = r { NSLog("nixxa.link op 0x%02x: %@", op, e.message) }
    }
  }

  private func newQueue() {
    queue?.close()
    let q = MessageQueue(
      sched: { ms, run in
        let w = DispatchWorkItem(block: run)
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(ms), execute: w)
        return { w.cancel() }
      },
      transport: { [weak self] frame, done in
        guard let self = self else { return done(false) }
        self.write(frame, done)
      }
    )
    queue = q
  }

  private func write(_ frame: Data, _ done: @escaping (Bool) -> Void) {
    guard let p = device, let rx = rxChar, p.state == .connected else {
      done(false)
      return
    }
    // ATT_MTU - 3 (what fits one write request); capped at the 512-byte
    // attribute maximum. The writes are still acknowledged one by one.
    let size = max(20, min(p.maximumWriteValueLength(for: .withoutResponse), Proto.attMax))
    writeChunks = Proto.chunk(frame, size: size)
    writeDone = done
    writeNext(p, rx)
  }

  private func writeNext(_ p: CBPeripheral, _ rx: CBCharacteristic) {
    if writeChunks.isEmpty {
      let d = writeDone
      writeDone = nil
      d?(true)
      return
    }
    let c = writeChunks.removeFirst()
    p.writeValue(c, for: rx, type: .withResponse)
  }

  private func failWrites() {
    writeChunks.removeAll()
    let d = writeDone
    writeDone = nil
    d?(false)
  }

  /// Drop the link state; with `cancel`, also disconnect our use of the peripheral.
  private func teardown(cancel: Bool) {
    if let s = setup {
      setup = nil
      s.timer?.cancel()
    }
    queue?.close()
    queue = nil
    failWrites()
    reasm.reset()
    ready = false
    infoChar = nil
    rxChar = nil
    txChar = nil
    if reconnectScan || pairFlow?.scanning == true, let c = central, c.state == .poweredOn { c.stopScan() }
    reconnectScan = false
    if cancel, let p = device, let c = central, p.state != .disconnected {
      c.cancelPeripheralConnection(p)
    }
  }

  private func cancelReconnect() {
    reconnectWork?.cancel()
    reconnectWork = nil
  }

  private func forgetLocal() {
    cancelReconnect()
    teardown(cancel: true)
    device = nil
    PairingStore.clear()
    pairing = nil
    fw = nil
    chip = nil
    publish()
  }

  // MARK: Pairing

  private func startPairScan(_ flow: PairFlow) {
    guard let c = central else { return }
    let name = "NIXXA-\(flow.qr.deviceId)"
    // ANCS can keep the bunny connected at the system level, and then it doesn't advertise.
    if let p = c.retrieveConnectedPeripherals(withServices: [NixxaLink.serviceId]).first(where: { $0.name == name }) {
      connectForPair(flow, p)
      return
    }
    flow.scanning = true
    c.scanForPeripherals(withServices: [NixxaLink.serviceId], options: nil)
    flow.timer = after(15_000) { [weak self] in
      self?.failPair(flow, LinkError(code: "NOT_FOUND", message: "No bunny named \(name) nearby. Is it on and showing its QR code?"))
    }
  }

  private func connectForPair(_ flow: PairFlow, _ p: CBPeripheral) {
    guard let c = central else { return }
    flow.timer?.cancel()
    device = p
    p.delegate = self
    flow.timer = after(20_000) { [weak self] in
      guard let self = self, self.setup == nil else { return }
      self.failPair(flow, LinkError(code: "NOT_FOUND", message: "Couldn't connect to NIXXA-\(flow.qr.deviceId). Move closer and try again."))
    }
    if p.state == .connected {
      flow.timer?.cancel()
      flow.timer = nil
      startSetup(p)
    } else {
      c.connect(p, options: nil)
    }
  }

  private func failPair(_ flow: PairFlow, _ e: LinkError) {
    guard pairFlow === flow else { return }
    teardown(cancel: true)
    if pairing == nil || device?.identifier.uuidString != pairing?.peripheralId { device = nil }
    endPair(flow, .failure(e))
    ensureConnection() // back to the previously paired bunny, if any
  }

  private func endPair(_ flow: PairFlow, _ r: Result<Status, LinkError>) {
    flow.timer?.cancel()
    flow.timer = nil
    if pairFlow === flow { pairFlow = nil }
    publish()
    flow.cb(r.map { _ in self.status })
  }

  // MARK: Reconnect

  /// Keep the paired bunny connected: a pending connect() (no timeout on iOS),
  /// or a background-capable scan for our service if iOS lost the peripheral.
  private func ensureConnection() {
    defer { publish() }
    guard pairFlow == nil, reconnectWork == nil, let pr = pairing, let c = central, c.state == .poweredOn else { return }
    if let p = device, p.identifier.uuidString == pr.peripheralId {
      switch p.state {
      case .connected:
        if setup == nil && !ready { startSetup(p) }
        return
      case .connecting:
        return
      default:
        p.delegate = self
        c.connect(p, options: nil)
        return
      }
    }
    guard let uuid = UUID(uuidString: pr.peripheralId) else { return }
    let p = c.retrievePeripherals(withIdentifiers: [uuid]).first
      ?? c.retrieveConnectedPeripherals(withServices: [NixxaLink.serviceId]).first(where: { $0.identifier == uuid })
    if let p = p {
      device = p
      p.delegate = self
      if p.state == .connected {
        startSetup(p)
      } else {
        c.connect(p, options: nil)
      }
    } else if !reconnectScan {
      reconnectScan = true
      c.scanForPeripherals(withServices: [NixxaLink.serviceId], options: nil)
    }
  }

  private func scheduleReconnect() {
    cancelReconnect()
    let ms = backoffMs
    backoffMs = min(backoffMs * 2, 30_000)
    reconnectWork = after(ms) { [weak self] in
      self?.reconnectWork = nil
      self?.ensureConnection()
    }
    publish()
  }

  // MARK: Setup

  private func startSetup(_ p: CBPeripheral) {
    let s: Setup
    if let flow = pairFlow {
      s = Setup(key: flow.qr.key, expectedId: flow.qr.deviceId, isPairing: true) { [weak self] r in
        self?.pairSetupDone(flow, p, r)
      }
    } else if let pr = pairing, let key = pr.key {
      s = Setup(key: key, expectedId: pr.deviceId, isPairing: false) { [weak self] r in
        self?.reconnectSetupDone(r)
      }
    } else {
      return
    }
    teardown(cancel: false)
    setup = s
    configExt = true
    newQueue()
    p.delegate = self
    publish()
    step(s, .discover, ms: 10_000)
    p.discoverServices([NixxaLink.serviceId])
  }

  private func step(_ s: Setup, _ st: Step, ms: Int) {
    s.step = st
    s.timer?.cancel()
    s.timer = after(ms) { [weak self] in
      let e: LinkError
      if s.isPairing && (st == .notify || st == .info) {
        e = LinkError(code: "BOND_FAILED", message: "Bluetooth pairing didn't finish. Tap Pair when iOS asks, then try again.")
      } else {
        e = LinkError(code: "TIMEOUT", message: "The bunny didn't answer in time")
      }
      self?.setupFailed(s, e)
    }
  }

  /// Steps that touch encrypted characteristics: their errors usually mean pairing trouble.
  private func bleError(_ s: Setup, _ error: Error) -> LinkError {
    let ns = error as NSError
    let name = "NIXXA-\(s.expectedId)"
    if ns.domain == CBErrorDomain && ns.code == CBError.Code.peerRemovedPairingInformation.rawValue {
      return LinkError(
        code: "BOND_FAILED",
        message: "This iPhone has an old pairing for \(name). In Settings > Bluetooth, tap (i) next to it, choose Forget This Device, then try again."
      )
    }
    if s.isPairing && (s.step == .notify || s.step == .info) {
      return LinkError(code: "BOND_FAILED", message: "Bluetooth pairing failed or was declined (\(ns.localizedDescription))")
    }
    return .notConnected(ns.localizedDescription)
  }

  private func setupFailed(_ s: Setup, _ e: LinkError) {
    guard setup === s else { return }
    NSLog("nixxa.link setup failed: %@ %@", e.code, e.message)
    teardown(cancel: true)
    s.done(.failure(e))
  }

  private func req(_ s: Setup, _ op: UInt8, _ payload: Data,
                   onError: ((LinkError) -> Bool)? = nil, _ next: @escaping (Data) -> Void) {
    guard let q = queue else {
      setupFailed(s, .notConnected())
      return
    }
    q.enqueue(op, payload) { [weak self] r in
      guard let self = self, self.setup === s else { return }
      switch r {
      case .success(let d):
        next(d)
      case .failure(let e):
        if let h = onError, h(e) { return }
        self.setupFailed(s, e)
      }
    }
  }

  private func onInfo(_ s: Setup, _ data: Data) {
    guard let info = Info.parse(data) else {
      return setupFailed(s, LinkError(code: "DEVICE_ERROR", message: "Bad INFO from the bunny", deviceStatus: Proto.stBadMsg))
    }
    if info.proto != Proto.protoVersion {
      return setupFailed(s, LinkError(code: "DEVICE_ERROR", message: "Unsupported protocol \(info.proto); update the app or firmware", deviceStatus: Proto.stBadMsg))
    }
    if info.deviceId != s.expectedId {
      return setupFailed(s, LinkError(code: "NOT_FOUND", message: "Found \(info.deviceId), expected \(s.expectedId)"))
    }
    step(s, .messages, ms: 30_000)
    queue?.open()
    reasm.reset()
    let tag = Proto.authTag(key: s.key, nonce: info.nonce)
    req(s, Proto.opAuth, tag, onError: { [weak self] e in
      guard e.deviceStatus == Proto.stAuthFailed else { return false }
      self?.setupFailed(s, LinkError(code: "AUTH_FAILED", message: "The bunny rejected the key; scan its QR code again"))
      return true
    }) { [weak self] _ in
      guard let self = self else { return }
      self.req(s, Proto.opSetTime, Proto.setTimeNow()) { [weak self] _ in
        self?.sendSetupConfig(s) { [weak self] in
          self?.sendSetupDnd(s) { [weak self] in self?.finishSetup(s, info) }
        }
      }
    }
  }

  /// SET_NOTIF_CONFIG with the ext byte; firmware without ANCS refuses it
  /// (BAD_MSG), so retry the classic layout once.
  private func sendSetupConfig(_ s: Setup, _ then: @escaping () -> Void) {
    req(s, Proto.opSetNotifConfig, Proto.notifConfig(config, ext: true), onError: { [weak self] e in
      guard let self = self, e.deviceStatus == Proto.stBadMsg else { return false }
      self.configExt = false
      self.req(s, Proto.opSetNotifConfig, Proto.notifConfig(self.config, ext: false)) { _ in then() }
      return true
    }) { _ in then() }
  }

  private func sendSetupDnd(_ s: Setup, _ then: @escaping () -> Void) {
    lastDnd = nil
    guard config.followDnd, let on = focusOn() else { return then() }
    req(s, Proto.opSetDnd, Proto.setDnd(on)) { [weak self] _ in
      self?.lastDnd = on
      then()
    }
  }

  private func finishSetup(_ s: Setup, _ info: Info) {
    guard setup === s else { return }
    s.timer?.cancel()
    setup = nil
    fw = info.fwVersion
    chip = info.chipName
    ready = true
    backoffMs = 1000
    s.done(.success(()))
    publish()
  }

  private func pairSetupDone(_ flow: PairFlow, _ p: CBPeripheral, _ r: Result<Void, LinkError>) {
    switch r {
    case .success:
      let pr = Pairing(peripheralId: p.identifier.uuidString, deviceId: flow.qr.deviceId,
                       keyHex: flow.qr.key.map { String(format: "%02X", $0) }.joined())
      if !PairingStore.save(pr) { NSLog("nixxa.link: saving the pairing to the Keychain failed") }
      pairing = pr
      endPair(flow, .success(status))
      if config.followDnd { requestFocusAuthorization() }
    case .failure(let e):
      failPair(flow, e)
    }
  }

  private func reconnectSetupDone(_ r: Result<Void, LinkError>) {
    guard case .failure(let e) = r else { return }
    if e.code == "AUTH_FAILED" {
      // The bunny was reflashed or unpaired elsewhere: K is gone for good.
      forgetLocal()
      return
    }
    scheduleReconnect()
  }

  // MARK: Focus (Do Not Disturb)

  private func focusOn() -> Bool? {
    let c = INFocusStatusCenter.default
    guard c.authorizationStatus == .authorized else { return nil }
    return c.focusStatus.isFocused
  }

  private func requestFocusAuthorization() {
    let c = INFocusStatusCenter.default
    guard c.authorizationStatus == .notDetermined else { return }
    c.requestAuthorization { [weak self] _ in
      DispatchQueue.main.async { self?.dndCheck() }
    }
  }

  private func dndCheck() {
    guard ready, config.followDnd, let on = focusOn(), on != lastDnd else { return }
    lastDnd = on
    fire(Proto.opSetDnd, Proto.setDnd(on))
  }
}

// MARK: - CBCentralManagerDelegate

extension NixxaLink: CBCentralManagerDelegate {
  func centralManagerDidUpdateState(_ central: CBCentralManager) {
    let st = central.state
    if st != .unknown && st != .resetting {
      let w = stateWaiters
      stateWaiters.removeAll()
      for f in w { f(st) }
    }
    if st == .poweredOn {
      ensureConnection()
    } else {
      if let f = pairFlow, st != .unknown && st != .resetting {
        failPair(f, LinkError(code: "BT_OFF", message: "Bluetooth is off"))
      }
      cancelReconnect()
      teardown(cancel: false)
      publish()
    }
  }

  func centralManager(_ central: CBCentralManager, willRestoreState dict: [String: Any]) {
    if pairing == nil { pairing = PairingStore.load() }
    guard let pr = pairing, let ps = dict[CBCentralManagerRestoredStatePeripheralsKey] as? [CBPeripheral] else { return }
    if let p = ps.first(where: { $0.identifier.uuidString == pr.peripheralId }) {
      device = p
      p.delegate = self
    }
  }

  func centralManager(_ central: CBCentralManager, didDiscover peripheral: CBPeripheral,
                      advertisementData: [String: Any], rssi RSSI: NSNumber) {
    let name = (advertisementData[CBAdvertisementDataLocalNameKey] as? String) ?? peripheral.name
    if let flow = pairFlow, flow.scanning {
      guard name == "NIXXA-\(flow.qr.deviceId)" else { return }
      flow.scanning = false
      central.stopScan()
      connectForPair(flow, peripheral)
      return
    }
    if reconnectScan, let pr = pairing {
      guard peripheral.identifier.uuidString == pr.peripheralId || name == "NIXXA-\(pr.deviceId)" else { return }
      reconnectScan = false
      central.stopScan()
      self.device = peripheral
      peripheral.delegate = self
      central.connect(peripheral, options: nil)
    }
  }

  func centralManager(_ central: CBCentralManager, didConnect peripheral: CBPeripheral) {
    guard peripheral === self.device else { return }
    if let flow = pairFlow {
      flow.timer?.cancel()
      flow.timer = nil
    }
    startSetup(peripheral)
  }

  func centralManager(_ central: CBCentralManager, didFailToConnect peripheral: CBPeripheral, error: Error?) {
    guard peripheral === self.device else { return }
    if let flow = pairFlow {
      failPair(flow, LinkError(code: "NOT_FOUND", message: "Couldn't connect to NIXXA-\(flow.qr.deviceId): \(error?.localizedDescription ?? "unknown error")"))
      return
    }
    teardown(cancel: false)
    scheduleReconnect()
  }

  func centralManager(_ central: CBCentralManager, didDisconnectPeripheral peripheral: CBPeripheral, error: Error?) {
    guard peripheral === self.device else { return }
    NSLog("nixxa.link disconnected: %@", error?.localizedDescription ?? "-")
    if let s = setup {
      let e: LinkError = s.isPairing && (s.step == .notify || s.step == .info)
        ? LinkError(code: "BOND_FAILED", message: "The bunny disconnected during pairing")
        : .notConnected("The bunny disconnected")
      setupFailed(s, e) // the setup's owner decides how to go on
      return
    }
    teardown(cancel: false)
    if pairFlow == nil && reconnectWork == nil {
      // A pending connect() has no timeout: iOS reconnects when the bunny is back.
      ensureConnection()
    }
    publish()
  }
}

// MARK: - CBPeripheralDelegate

extension NixxaLink: CBPeripheralDelegate {
  func peripheral(_ peripheral: CBPeripheral, didDiscoverServices error: Error?) {
    guard peripheral === self.device, let s = setup, s.step == .discover else { return }
    if let error = error { return setupFailed(s, bleError(s, error)) }
    guard let svc = peripheral.services?.first(where: { $0.uuid == NixxaLink.serviceId }) else {
      return setupFailed(s, LinkError(code: "NOT_FOUND", message: "That device isn't a nixxa bunny"))
    }
    peripheral.discoverCharacteristics([NixxaLink.infoId, NixxaLink.rxId, NixxaLink.txId], for: svc)
  }

  func peripheral(_ peripheral: CBPeripheral, didDiscoverCharacteristicsFor service: CBService, error: Error?) {
    guard peripheral === self.device, let s = setup, s.step == .discover, service.uuid == NixxaLink.serviceId else { return }
    if let error = error { return setupFailed(s, bleError(s, error)) }
    let chars = service.characteristics ?? []
    infoChar = chars.first { $0.uuid == NixxaLink.infoId }
    rxChar = chars.first { $0.uuid == NixxaLink.rxId }
    txChar = chars.first { $0.uuid == NixxaLink.txId }
    guard let tx = txChar, infoChar != nil, rxChar != nil else {
      return setupFailed(s, LinkError(code: "NOT_FOUND", message: "That device isn't a nixxa bunny"))
    }
    // TX and INFO need encryption: the first of them brings up the iOS pairing dialog.
    step(s, .notify, ms: s.isPairing ? 60_000 : 30_000)
    if tx.isNotifying {
      readInfo(peripheral, s)
    } else {
      peripheral.setNotifyValue(true, for: tx)
    }
  }

  private func readInfo(_ p: CBPeripheral, _ s: Setup) {
    guard let info = infoChar else { return setupFailed(s, .notConnected()) }
    step(s, .info, ms: s.isPairing ? 60_000 : 30_000)
    p.readValue(for: info)
  }

  func peripheral(_ peripheral: CBPeripheral, didUpdateNotificationStateFor characteristic: CBCharacteristic, error: Error?) {
    guard peripheral === self.device, let s = setup, s.step == .notify, characteristic.uuid == NixxaLink.txId else { return }
    if let error = error { return setupFailed(s, bleError(s, error)) }
    readInfo(peripheral, s)
  }

  func peripheral(_ peripheral: CBPeripheral, didUpdateValueFor characteristic: CBCharacteristic, error: Error?) {
    guard peripheral === self.device else { return }
    if characteristic.uuid == NixxaLink.txId {
      guard error == nil, let v = characteristic.value, let q = queue else { return }
      let nowMs = Int64(ProcessInfo.processInfo.systemUptime * 1000)
      for m in reasm.feed(v, nowMs: nowMs) { q.onMessage(m) }
    } else if characteristic.uuid == NixxaLink.infoId {
      guard let s = setup, s.step == .info else { return }
      if let error = error { return setupFailed(s, bleError(s, error)) }
      onInfo(s, characteristic.value ?? Data())
    }
  }

  func peripheral(_ peripheral: CBPeripheral, didWriteValueFor characteristic: CBCharacteristic, error: Error?) {
    guard peripheral === self.device, characteristic.uuid == NixxaLink.rxId else { return }
    if let error = error {
      NSLog("nixxa.link write failed: %@", error.localizedDescription)
      failWrites()
      return
    }
    writeNext(peripheral, characteristic)
  }
}
