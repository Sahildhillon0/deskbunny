// nixxa BLE protocol, pure Swift (Foundation + CryptoKit, no CoreBluetooth or
// Flutter) so RunnerTests can exercise it. Mirrors the Kotlin side
// (mobile/android/.../proto/Proto.kt, Notif.kt, MessageQueue.kt).
// Contract: docs/ARCHITECTURE.md "ESP32 + BLE (nixxa)" and "iOS (phase 3)".
import CryptoKit
import Foundation

enum Proto {
  static let serviceUUID = "4E580001-6E69-4878-A100-6E6978786100"
  static let infoUUID = "4E580002-6E69-4878-A100-6E6978786100"
  static let rxUUID = "4E580003-6E69-4878-A100-6E6978786100"
  static let txUUID = "4E580004-6E69-4878-A100-6E6978786100"

  static let protoVersion = 1
  static let hdr = 4
  static let infoSize = 41
  static let attMax = 512
  static let bodyMax = 512
  static let rulesMax = 32
  static let appIdMax = 64

  static let opAuth: UInt8 = 0x01
  static let opGetState: UInt8 = 0x02
  static let opSetTime: UInt8 = 0x03
  static let opListSlots: UInt8 = 0x10
  static let opPutSlot: UInt8 = 0x11
  static let opGetSlot: UInt8 = 0x12
  static let opDeleteSlot: UInt8 = 0x13
  static let opActivate: UInt8 = 0x14
  static let opSetNotifConfig: UInt8 = 0x20
  static let opNotify: UInt8 = 0x21
  static let opDismiss: UInt8 = 0x22
  static let opSetDnd: UInt8 = 0x23
  static let opListSeenApps: UInt8 = 0x24
  static let opUnpair: UInt8 = 0x7F

  static let stOk = 0
  static let stNotAuthed = 1
  static let stBadMsg = 2
  static let stBadSlot = 3
  static let stNoSpace = 4
  static let stBusy = 5
  static let stAuthFailed = 6

  static let kindMessage = 0
  static let kindCall = 1
  static let kindCallEnded = 2

  static func statusName(_ st: Int) -> String {
    switch st {
    case stOk: return "OK"
    case stNotAuthed: return "NOT_AUTHED"
    case stBadMsg: return "BAD_MSG"
    case stBadSlot: return "BAD_SLOT"
    case stNoSpace: return "NO_SPACE"
    case stBusy: return "BUSY"
    case stAuthFailed: return "AUTH_FAILED"
    default: return String(format: "0x%02X", st)
    }
  }

  /// op u8, seq u8, len u16 LE, payload.
  static func frame(op: UInt8, seq: UInt8, payload: Data) -> Data {
    precondition(payload.count <= 0xFFFF, "payload too large")
    var out = [UInt8]()
    out.reserveCapacity(hdr + payload.count)
    out.append(op)
    out.append(seq)
    out.append(UInt8(payload.count & 0xFF))
    out.append(UInt8((payload.count >> 8) & 0xFF))
    out.append(contentsOf: payload)
    return Data(out)
  }

  /// Split one message into GATT writes of at most `size` bytes.
  static func chunk(_ data: Data, size: Int) -> [Data] {
    precondition(size > 0)
    let b = [UInt8](data)
    if b.isEmpty { return [Data()] }
    return stride(from: 0, to: b.count, by: size).map { Data(b[$0..<min($0 + size, b.count)]) }
  }

  /// tag = first 16 bytes of HMAC-SHA256(k, nonce || "NIXXA-AUTH").
  static func authTag(key: Data, nonce: Data) -> Data {
    precondition(key.count == 16 && nonce.count == 16)
    var msg = Data(nonce)
    msg.append(Data("NIXXA-AUTH".utf8))
    let mac = HMAC<SHA256>.authenticationCode(for: msg, using: SymmetricKey(data: key))
    return Data(Array(mac)[0..<16])
  }

  static func setTime(unixSecs: Int64, tzOffsetMin: Int) -> Data {
    var w = LeWriter()
    w.u32(UInt32(truncatingIfNeeded: unixSecs))
    w.u16(Int(UInt16(truncatingIfNeeded: tzOffsetMin))) // i16
    return w.data
  }

  static func setTimeNow() -> Data {
    let now = Date()
    let tz = TimeZone.current.secondsFromGMT(for: now) / 60
    return setTime(unixSecs: Int64(now.timeIntervalSince1970), tzOffsetMin: tz)
  }

  static func dismiss(_ notifId: UInt32) -> Data {
    var w = LeWriter()
    w.u32(notifId)
    return w.data
  }

  static func setDnd(_ on: Bool) -> Data { Data([on ? 1 : 0]) }

  /// SET_NOTIF_CONFIG. At most 32 rules (shown rules first); rule mood 0xFF
  /// ("default") is sent as the default mood. With `ext`, the trailing
  /// `ext u8` (bit0 = forward calls) is appended; firmware older than the
  /// ANCS release rejects it with BAD_MSG, so the link retries without it.
  static func notifConfig(_ c: NotifConfig, ext: Bool = true) -> Data {
    var w = LeWriter()
    w.u8(c.enabled ? 1 : 0)
    w.u8(min(max(c.cardSecs, 2), 30))
    w.u8(c.defaultMood & 0xFF)
    w.u16(min(max(c.quietStartMin, 0), 1439))
    w.u16(min(max(c.quietEndMin, 0), 1439))
    w.u8(c.callsInQuiet ? 1 : 0)
    let valid = c.rules.filter { r in
      !r.appId.isEmpty && r.appId.utf8.count <= appIdMax && r.appId.unicodeScalars.allSatisfy { $0.value >= 0x20 && $0.value <= 0x7E }
    }
    // Stable partition: shown rules first, each group in the user's order.
    let rules = Array((valid.filter { $0.show } + valid.filter { !$0.show }).prefix(rulesMax))
    w.u8(rules.count)
    for r in rules {
      let id = [UInt8](r.appId.utf8)
      w.u8(id.count)
      w.raw(id)
      w.u8(r.mood == 0xFF ? (c.defaultMood & 0xFF) : (r.mood & 0xFF))
      w.u8(r.show ? 1 : 0)
    }
    if ext { w.u8(c.calls ? 1 : 0) }
    return w.data
  }

  /// NOTIFY. Strings are cut to their length fields (u8, body 512).
  static func notify(
    kind: Int, notifId: UInt32, mood: Int, appId: String, appName: String,
    title: String, body: String, icon: Data?
  ) -> Data {
    var w = LeWriter()
    w.u8(kind)
    w.u32(notifId)
    w.u8(mood & 0xFF)
    w.str8(appId)
    w.str8(appName)
    w.str8(title)
    let b = utf8Cut(body, max: bodyMax)
    w.u16(b.count)
    w.raw(b)
    if let icon = icon {
      // flag 1: 16x16 (32 bytes), flag 2: 32x32 (128 bytes)
      precondition(icon.count == 32 || icon.count == 128)
      w.u8(icon.count == 128 ? 2 : 1)
      w.raw([UInt8](icon))
    } else {
      w.u8(0)
    }
    return w.data
  }

  /// UTF-8 bytes of s, cut at a code-point boundary to at most max bytes.
  static func utf8Cut(_ s: String, max: Int) -> [UInt8] {
    let b = [UInt8](s.utf8)
    if b.count <= max { return b }
    var n = max
    while n > 0 && (b[n] & 0xC0) == 0x80 { n -= 1 } // don't split a sequence
    return Array(b[0..<n])
  }

  /// LIST_SEEN_APPS (0x24) reply after the status byte: count u8, then per
  /// app idLen u8, appId, nameLen u8, name. nil when malformed.
  static func parseSeenApps(_ data: Data) -> [SeenApp]? {
    let b = [UInt8](data)
    guard !b.isEmpty else { return nil }
    let count = Int(b[0])
    var pos = 1
    var out = [SeenApp]()
    for _ in 0..<count {
      guard pos < b.count else { return nil }
      let idLen = Int(b[pos]); pos += 1
      guard pos + idLen < b.count else { return nil }
      let id = String(decoding: b[pos..<pos + idLen], as: UTF8.self); pos += idLen
      let nameLen = Int(b[pos]); pos += 1
      guard pos + nameLen <= b.count else { return nil }
      let name = String(decoding: b[pos..<pos + nameLen], as: UTF8.self); pos += nameLen
      out.append(SeenApp(appId: id, name: name))
    }
    return out
  }

  struct LeWriter {
    private(set) var bytes = [UInt8]()
    var data: Data { Data(bytes) }
    mutating func u8(_ v: Int) { bytes.append(UInt8(truncatingIfNeeded: v)) }
    mutating func u16(_ v: Int) {
      bytes.append(UInt8(truncatingIfNeeded: v))
      bytes.append(UInt8(truncatingIfNeeded: v >> 8))
    }
    mutating func u32(_ v: UInt32) {
      for i in 0..<4 { bytes.append(UInt8(truncatingIfNeeded: v >> (8 * UInt32(i)))) }
    }
    mutating func raw(_ b: [UInt8]) { bytes.append(contentsOf: b) }
    mutating func str8(_ s: String) {
      let b = Proto.utf8Cut(s, max: 255)
      u8(b.count)
      raw(b)
    }
  }
}

struct SeenApp: Equatable {
  let appId: String
  let name: String
}

/// Codes match LinkException in mobile/lib/link/device_link.dart.
struct LinkError: Error, Equatable {
  let code: String
  let message: String
  var deviceStatus: Int? = nil

  static func notConnected(_ m: String = "Not connected to the bunny") -> LinkError {
    LinkError(code: "NOT_CONNECTED", message: m)
  }
}

/// INFO characteristic (41 bytes).
struct Info: Equatable {
  let proto: Int
  let chip: Int
  let paired: Bool
  let slots: Int
  let maxPayload: Int
  let deviceId: String
  let nonce: Data
  let fwVersion: String

  var chipName: String {
    switch chip {
    case 0: return "esp32"
    case 1: return "esp32c3"
    case 2: return "esp32s3"
    default: return "unknown"
    }
  }

  static func parse(_ data: Data) -> Info? {
    let b = [UInt8](data)
    guard b.count >= Proto.infoSize else { return nil }
    let id = b[6..<9].map { String(format: "%02X", $0) }.joined()
    var end = 25
    while end < 41 && b[end] != 0 { end += 1 }
    return Info(
      proto: Int(b[0]), chip: Int(b[1]), paired: b[2] != 0, slots: Int(b[3]),
      maxPayload: Int(b[4]) | (Int(b[5]) << 8), deviceId: id,
      nonce: Data(b[9..<25]),
      fwVersion: String(decoding: b[25..<end], as: UTF8.self)
    )
  }
}

/// QR payload "NIXXA1:<ID>:<K as 32 uppercase hex>".
struct PairingQr: Equatable {
  let deviceId: String
  let key: Data

  static func parse(_ text: String) -> PairingQr? {
    let t = text.trimmingCharacters(in: .whitespacesAndNewlines).uppercased()
    let parts = t.split(separator: ":", omittingEmptySubsequences: false)
    guard parts.count == 3, parts[0] == "NIXXA1" else { return nil }
    let id = String(parts[1]), hex = String(parts[2])
    guard id.count == 6, isHex(id), hex.count == 32, isHex(hex), let k = unhex(hex) else { return nil }
    return PairingQr(deviceId: id, key: k)
  }

  static func isHex(_ s: String) -> Bool {
    s.unicodeScalars.allSatisfy { c in
      let v = c.value
      return (v >= 0x30 && v <= 0x39) || (v >= 0x41 && v <= 0x46) // 0-9, A-F
    }
  }

  static func unhex(_ s: String) -> Data? {
    let c = Array(s.utf8)
    guard c.count % 2 == 0 else { return nil }
    var out = [UInt8]()
    var i = 0
    while i < c.count {
      guard let v = UInt8(String(decoding: c[i..<i + 2], as: UTF8.self), radix: 16) else { return nil }
      out.append(v)
      i += 2
    }
    return Data(out)
  }
}

/// Reassembles device -> phone messages from TX notifications by the len
/// field. Several messages in one notification are all returned. A partial
/// message older than `gapMs` is abandoned when the next notification arrives.
final class Reassembler {
  struct Msg: Equatable {
    let op: UInt8
    let seq: UInt8
    let payload: Data
  }

  private var buf = [UInt8]()
  private var lastMs: Int64 = 0
  private let gapMs: Int64

  init(gapMs: Int64 = 1000) { self.gapMs = gapMs }

  func reset() { buf.removeAll() }

  func feed(_ data: Data, nowMs: Int64) -> [Msg] {
    if !buf.isEmpty && nowMs - lastMs >= gapMs { buf.removeAll() }
    lastMs = nowMs
    buf.append(contentsOf: data)
    var out = [Msg]()
    var pos = 0
    while buf.count - pos >= Proto.hdr {
      let len = Int(buf[pos + 2]) | (Int(buf[pos + 3]) << 8)
      if buf.count - pos < Proto.hdr + len { break }
      out.append(Msg(op: buf[pos], seq: buf[pos + 1], payload: Data(buf[(pos + Proto.hdr)..<(pos + Proto.hdr + len)])))
      pos += Proto.hdr + len
    }
    if pos > 0 { buf.removeFirst(pos) }
    return out
  }
}

/// Stop-and-wait request queue (one message in flight, FIFO, reply timeout).
/// Not thread-safe: every method must be called on the same queue (main).
final class MessageQueue {
  /// Run the closure after `ms`; the returned closure cancels it.
  typealias Scheduler = (_ ms: Int, _ run: @escaping () -> Void) -> () -> Void
  /// Write one framed message (chunked by the transport); done(false) on a GATT failure.
  typealias Transport = (_ frame: Data, _ done: @escaping (Bool) -> Void) -> Void
  typealias Callback = (Result<Data, LinkError>) -> Void

  private final class Pending {
    let op: UInt8
    let payload: Data
    let cb: Callback
    init(op: UInt8, payload: Data, cb: @escaping Callback) {
      self.op = op
      self.payload = payload
      self.cb = cb
    }
  }

  private let sched: Scheduler
  private let transport: Transport
  private let timeoutMs: Int
  private var queue = [Pending]()
  private var inflight: Pending?
  private var inflightSeq: UInt8 = 0
  private var cancelTimer: (() -> Void)?
  private var seq: UInt8 = 0
  private var generation = 0

  /// Closed queues refuse new requests with NOT_CONNECTED.
  private(set) var isOpen = false
  var busy: Bool { inflight != nil || !queue.isEmpty }

  init(sched: @escaping Scheduler, transport: @escaping Transport, timeoutMs: Int = 5000) {
    self.sched = sched
    self.transport = transport
    self.timeoutMs = timeoutMs
  }

  func open() { isOpen = true }

  func enqueue(_ op: UInt8, _ payload: Data, _ cb: @escaping Callback) {
    if !isOpen {
      cb(.failure(.notConnected()))
      return
    }
    queue.append(Pending(op: op, payload: payload, cb: cb))
    pump()
  }

  /// Feed every reassembled device -> phone message here.
  func onMessage(_ m: Reassembler.Msg) {
    guard let p = inflight else { return }
    if m.op != (p.op | 0x80) || m.seq != inflightSeq { return } // stale or unsolicited
    let b = [UInt8](m.payload)
    if b.isEmpty {
      finish(.failure(LinkError(code: "DEVICE_ERROR", message: "Empty reply", deviceStatus: Proto.stBadMsg)))
      return
    }
    let st = Int(b[0])
    if st == Proto.stOk {
      finish(.success(Data(b[1...])))
    } else {
      finish(.failure(LinkError(code: "DEVICE_ERROR", message: "Device status \(Proto.statusName(st))", deviceStatus: st)))
    }
  }

  /// Fail the in-flight and queued requests and refuse new ones.
  func close(code: String = "NOT_CONNECTED", message: String = "Disconnected from the bunny") {
    isOpen = false
    generation += 1
    cancelTimer?()
    cancelTimer = nil
    var all = [Pending]()
    if let p = inflight { all.append(p) }
    all.append(contentsOf: queue)
    inflight = nil
    queue.removeAll()
    for p in all { p.cb(.failure(LinkError(code: code, message: message))) }
  }

  private func pump() {
    guard inflight == nil, !queue.isEmpty, isOpen else { return }
    let p = queue.removeFirst()
    inflight = p
    seq = seq &+ 1
    inflightSeq = seq
    generation += 1
    let gen = generation
    cancelTimer = sched(timeoutMs) { [weak self] in
      guard let self = self, gen == self.generation, self.inflight === p else { return }
      self.finish(.failure(LinkError(code: "TIMEOUT", message: "No reply from the bunny within \(self.timeoutMs / 1000) s")))
    }
    transport(Proto.frame(op: p.op, seq: inflightSeq, payload: p.payload)) { [weak self] ok in
      guard let self = self, !ok, gen == self.generation, self.inflight === p else { return }
      self.finish(.failure(.notConnected("Write to the bunny failed")))
    }
  }

  private func finish(_ r: Result<Data, LinkError>) {
    guard let p = inflight else { return }
    inflight = nil
    generation += 1
    cancelTimer?()
    cancelTimer = nil
    p.cb(r)
    pump()
  }
}

// MARK: - Notification config (same map keys as NotifConfig in device_link.dart)

struct NotifRule: Codable, Equatable {
  var appId: String
  var mood: Int = 0xFF
  var show: Bool = true

  func toMap() -> [String: Any] { ["appId": appId, "mood": mood, "show": show] }

  static func fromMap(_ m: [String: Any]) -> NotifRule? {
    guard let id = m["appId"] as? String,
          let mood = (m["mood"] as? NSNumber)?.intValue,
          let show = (m["show"] as? NSNumber)?.boolValue else { return nil }
    return NotifRule(appId: id, mood: mood, show: show)
  }
}

struct NotifConfig: Codable, Equatable {
  var enabled = true
  var cardSecs = 6
  var defaultMood = 5
  var quietStartMin = 0
  var quietEndMin = 0
  var callsInQuiet = false
  var followDnd = true
  var calls = true
  var rules: [NotifRule] = []

  func toMap() -> [String: Any] {
    [
      "enabled": enabled, "cardSecs": cardSecs, "defaultMood": defaultMood,
      "quietStartMin": quietStartMin, "quietEndMin": quietEndMin,
      "callsInQuiet": callsInQuiet, "followDnd": followDnd, "calls": calls,
      "rules": rules.map { $0.toMap() },
    ]
  }

  static func fromMap(_ m: [String: Any]) -> NotifConfig? {
    func int(_ k: String) -> Int? { (m[k] as? NSNumber)?.intValue }
    func bool(_ k: String) -> Bool? { (m[k] as? NSNumber)?.boolValue }
    guard let enabled = bool("enabled"), let cardSecs = int("cardSecs"),
          let defaultMood = int("defaultMood"), let qs = int("quietStartMin"),
          let qe = int("quietEndMin"), let ciq = bool("callsInQuiet"),
          let followDnd = bool("followDnd"), let calls = bool("calls"),
          let list = m["rules"] as? [Any] else { return nil }
    var rules = [NotifRule]()
    for item in list {
      guard let rm = item as? [String: Any], let r = NotifRule.fromMap(rm) else { return nil }
      rules.append(r)
    }
    return NotifConfig(
      enabled: enabled, cardSecs: cardSecs, defaultMood: defaultMood,
      quietStartMin: qs, quietEndMin: qe, callsInQuiet: ciq,
      followDnd: followDnd, calls: calls, rules: rules
    )
  }
}

// MARK: - App list (listApps on iOS)

enum AppCatalogue {
  /// Common bundle IDs offered even before the bunny has seen them.
  static let common: [(appId: String, label: String)] = [
    ("com.apple.MobileSMS", "Messages"),
    ("com.apple.mobilephone", "Phone"),
    ("com.apple.mobilemail", "Mail"),
    ("com.apple.mobilecal", "Calendar"),
    ("net.whatsapp.WhatsApp", "WhatsApp"),
    ("ph.telegra.Telegraph", "Telegram"),
    ("org.whispersystems.signal", "Signal"),
    ("com.google.Gmail", "Gmail"),
    ("com.microsoft.Office.Outlook", "Outlook"),
    ("com.tinyspeck.chatlyio", "Slack"),
    ("com.microsoft.skype.teams", "Teams"),
    ("com.hammerandchisel.discord", "Discord"),
    ("com.burbn.instagram", "Instagram"),
    ("com.facebook.Messenger", "Messenger"),
    ("com.toyopagroup.picaboo", "Snapchat"),
    ("com.atebits.Tweetie2", "X"),
    ("com.linkedin.LinkedIn", "LinkedIn"),
  ]

  /// Seen apps (newest first, from the bunny) merged with the catalogue:
  /// one entry per appId, the bunny's display name winning when it has one,
  /// sorted by label (case-insensitive, then appId).
  static func merge(seen: [SeenApp]) -> [(appId: String, label: String)] {
    var labels = [String: String]()
    for c in common { labels[c.appId] = c.label }
    for s in seen where !s.appId.isEmpty {
      let name = s.name.trimmingCharacters(in: .whitespacesAndNewlines)
      if !name.isEmpty {
        labels[s.appId] = name
      } else if labels[s.appId] == nil {
        labels[s.appId] = s.appId
      }
    }
    return labels.map { (appId: $0.key, label: $0.value) }.sorted { a, b in
      let la = a.label.lowercased(), lb = b.label.lowercased()
      return la != lb ? la < lb : a.appId < b.appId
    }
  }
}
