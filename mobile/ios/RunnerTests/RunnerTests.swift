// Unit tests for the pure-Swift protocol layer (Runner/NixxaProto.swift).
// Same vectors as the Kotlin ProtoTest/MessageQueue tests and
// firmware/nixxa_esp32/tests/test_core.c.
import XCTest
@testable import Runner

private func hex(_ d: Data) -> String { d.map { String(format: "%02x", $0) }.joined() }
private func bytes(_ v: Int...) -> Data { Data(v.map { UInt8(truncatingIfNeeded: $0) }) }
private func ascii(_ s: String) -> Data { Data(s.utf8) }

class RunnerTests: XCTestCase {
  func testAuthTagMatchesFirmwareVector() {
    // firmware/nixxa_esp32/tests/test_core.c test_auth
    let k = Data((0..<16).map { UInt8($0) })
    let nonce = Data((0..<16).map { UInt8(0x10 + $0) })
    XCTAssertEqual(hex(Proto.authTag(key: k, nonce: nonce)), "8b0bfcfaf44904dcf18843ba7721e362")
  }

  func testInfoParse() {
    var b = [UInt8](repeating: 0, count: 41)
    b[0] = 1; b[1] = 2; b[2] = 1; b[3] = 16; b[4] = 0x40; b[5] = 0x30
    b[6] = 0xA1; b[7] = 0xB2; b[8] = 0xC3
    for i in 0..<16 { b[9 + i] = UInt8(100 + i) }
    for (i, c) in "0.2.0".utf8.enumerated() { b[25 + i] = c }
    let info = Info.parse(Data(b))!
    XCTAssertEqual(info.proto, 1)
    XCTAssertEqual(info.chipName, "esp32s3")
    XCTAssertTrue(info.paired)
    XCTAssertEqual(info.slots, 16)
    XCTAssertEqual(info.maxPayload, 12352)
    XCTAssertEqual(info.deviceId, "A1B2C3")
    XCTAssertEqual(info.nonce, Data((0..<16).map { UInt8(100 + $0) }))
    XCTAssertEqual(info.fwVersion, "0.2.0")
    XCTAssertNil(Info.parse(Data(count: 40)))
    for (i, c) in "0123456789ABCDEF".utf8.enumerated() { b[25 + i] = c }
    XCTAssertEqual(Info.parse(Data(b))!.fwVersion, "0123456789ABCDEF")
    // A slice with a non-zero start index parses the same.
    let padded = Data([0xEE] + b)
    XCTAssertEqual(Info.parse(padded[1...])?.deviceId, "A1B2C3")
  }

  func testQrParse() {
    let q = PairingQr.parse("NIXXA1:A1B2C3:F0F1F2F3F4F5F6F7F8F9FAFBFCFDFEFF")!
    XCTAssertEqual(q.deviceId, "A1B2C3")
    XCTAssertEqual(q.key, Data((0..<16).map { UInt8(0xF0 + $0) }))
    XCTAssertNotNil(PairingQr.parse("  nixxa1:a1b2c3:f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff\n"))
    XCTAssertNil(PairingQr.parse("NIXXA1:A1B2C3:F0F1"))
    XCTAssertNil(PairingQr.parse("NIXXA2:A1B2C3:F0F1F2F3F4F5F6F7F8F9FAFBFCFDFEFF"))
    XCTAssertNil(PairingQr.parse("NIXXA1:A1B2CG:F0F1F2F3F4F5F6F7F8F9FAFBFCFDFEFF"))
    XCTAssertNil(PairingQr.parse("https://example.com"))
  }

  func testFrameAndChunk() {
    let f = Proto.frame(op: 0x21, seq: 9, payload: Data((0..<296).map { UInt8(truncatingIfNeeded: $0) }))
    XCTAssertEqual(f.count, 300)
    XCTAssertEqual([UInt8](f.prefix(4)), [0x21, 9, 0x28, 0x01])
    let chunks = Proto.chunk(f, size: 182)
    XCTAssertEqual(chunks.map { $0.count }, [182, 118])
    XCTAssertEqual(chunks.reduce(Data(), +), f)
    XCTAssertEqual(Proto.chunk(Proto.frame(op: 2, seq: 1, payload: Data()), size: 20).count, 1)
    XCTAssertEqual(Proto.chunk(Proto.frame(op: 0x11, seq: 1, payload: Data(count: 12352)), size: 509).count, 25)
  }

  func testReassembly() {
    let r = Reassembler()
    let msg = Proto.frame(op: 0x92, seq: 5, payload: Data((0..<300).map { UInt8(truncatingIfNeeded: $0) }))
    XCTAssertTrue(r.feed(msg.subdata(in: 0..<1), nowMs: 0).isEmpty) // split header
    XCTAssertTrue(r.feed(msg.subdata(in: 1..<3), nowMs: 1).isEmpty)
    XCTAssertTrue(r.feed(msg.subdata(in: 3..<200), nowMs: 2).isEmpty)
    let out = r.feed(msg.subdata(in: 200..<msg.count), nowMs: 3)
    XCTAssertEqual(out.count, 1)
    XCTAssertEqual(out[0].op, 0x92)
    XCTAssertEqual(out[0].seq, 5)
    XCTAssertEqual(out[0].payload, Data((0..<300).map { UInt8(truncatingIfNeeded: $0) }))

    // Two messages in one notification, the second split.
    let a = Proto.frame(op: 0x81, seq: 1, payload: bytes(0))
    let b = Proto.frame(op: 0x82, seq: 2, payload: bytes(0, 1, 2, 3, 4, 5))
    XCTAssertEqual(r.feed(a + b.subdata(in: 0..<5), nowMs: 10).map { $0.op }, [0x81])
    let second = r.feed(b.subdata(in: 5..<b.count), nowMs: 11)
    XCTAssertEqual(second.map { $0.op }, [0x82])
    XCTAssertEqual(second[0].payload, bytes(0, 1, 2, 3, 4, 5))

    // A stale half message is abandoned after the gap.
    XCTAssertTrue(r.feed(b.subdata(in: 0..<6), nowMs: 100).isEmpty)
    XCTAssertEqual(r.feed(a, nowMs: 2000).map { $0.op }, [0x81])
  }

  func testSetTimeDismissDnd() {
    XCTAssertEqual(Proto.setTime(unixSecs: 0x1234_5678, tzOffsetMin: 330), bytes(0x78, 0x56, 0x34, 0x12, 0x4A, 0x01))
    XCTAssertEqual(Proto.setTime(unixSecs: 0x8000_0000, tzOffsetMin: -60), bytes(0x00, 0x00, 0x00, 0x80, 0xC4, 0xFF))
    XCTAssertEqual(Proto.dismiss(0x1234_5678), bytes(0x78, 0x56, 0x34, 0x12))
    XCTAssertEqual(Proto.dismiss(0xFFFF_FFFF), bytes(0xFF, 0xFF, 0xFF, 0xFF))
    XCTAssertEqual(Proto.setDnd(true), bytes(1))
    XCTAssertEqual(Proto.setDnd(false), bytes(0))
  }

  func testNotifConfigMatchesFirmwareLayout() {
    // Mirrors test_core.c test_cfg, plus the trailing ext byte (bit0 = calls).
    var c = NotifConfig(
      enabled: true, cardSecs: 30, defaultMood: 0, quietStartMin: 22 * 60, quietEndMin: 7 * 60,
      callsInQuiet: true,
      rules: [NotifRule(appId: "com.whatsapp", mood: 6, show: true), NotifRule(appId: "com.slack", mood: 2, show: false)]
    )
    var want = bytes(1, 30, 0, 0x28, 0x05, 0xA4, 0x01, 1, 2)
    want += bytes(12) + ascii("com.whatsapp") + bytes(6, 1)
    want += bytes(9) + ascii("com.slack") + bytes(2, 0)
    XCTAssertEqual(Proto.notifConfig(c, ext: false), want)
    XCTAssertEqual(Proto.notifConfig(c), want + bytes(1))
    c.calls = false
    XCTAssertEqual(Proto.notifConfig(c), want + bytes(0))
  }

  func testNotifConfigLimits() {
    var rules = (0..<40).map { NotifRule(appId: "app.\($0)", mood: $0 == 0 ? 0xFF : 3, show: $0 % 2 == 0) }
    rules += [NotifRule(appId: String(repeating: "x", count: 65)), NotifRule(appId: ""), NotifRule(appId: "bad.é")]
    let p = [UInt8](Proto.notifConfig(NotifConfig(cardSecs: 99, defaultMood: 7, quietStartMin: 5000, rules: rules)))
    XCTAssertEqual(p[1], 30)
    XCTAssertEqual(Int(p[3]) | (Int(p[4]) << 8), 1439)
    XCTAssertEqual(p[8], 32)
    // Shown rules come first; mood 0xFF becomes the default mood.
    XCTAssertEqual(p[9], 5)
    XCTAssertEqual(String(decoding: p[10..<15], as: UTF8.self), "app.0")
    XCTAssertEqual(p[15], 7)
    XCTAssertEqual(p[16], 1)
    // Walk like nx_cfg_parse: 32 rules, then exactly the ext byte.
    var pos = 9
    var shown = 0
    for _ in 0..<32 {
      let n = Int(p[pos])
      XCTAssertTrue((1...64).contains(n))
      if p[pos + n + 2] == 1 { shown += 1 }
      pos += 1 + n + 2
    }
    XCTAssertEqual(shown, 20)
    XCTAssertEqual(pos, p.count - 1)
    XCTAssertEqual(p[pos], 1)
  }

  func testNotifyMatchesFirmwareLayout() {
    let icon = Data((0..<32).map { UInt8($0) })
    let p = Proto.notify(kind: 0, notifId: 0x1234_5678, mood: 0xFF, appId: "com.x", appName: "X",
                         title: "Hello", body: String(repeating: "b", count: 600), icon: icon)
    var want = bytes(0, 0x78, 0x56, 0x34, 0x12, 0xFF)
    want += bytes(5) + ascii("com.x") + bytes(1) + ascii("X") + bytes(5) + ascii("Hello")
    want += bytes(0x00, 0x02) + ascii(String(repeating: "b", count: 512)) + bytes(1) + icon
    XCTAssertEqual(p, want)

    let call = Proto.notify(kind: 1, notifId: 7, mood: 3, appId: "com.dialer", appName: "Phone",
                            title: "Mum", body: "", icon: nil)
    XCTAssertEqual(call, bytes(1, 7, 0, 0, 0, 3, 10) + ascii("com.dialer") + bytes(5) + ascii("Phone")
      + bytes(3) + ascii("Mum") + bytes(0, 0, 0))
  }

  func testUtf8CutKeepsSequencesWhole() {
    XCTAssertEqual(Proto.utf8Cut("aé", max: 2), [UInt8]("a".utf8))
    XCTAssertEqual(Proto.utf8Cut("aé", max: 3).count, 3)
    let p = [UInt8](Proto.notify(kind: 0, notifId: 0, mood: 0, appId: "a", appName: "b",
                                 title: String(repeating: "t", count: 300), body: "", icon: nil))
    XCTAssertEqual(p[6 + 2 + 2], 255)
  }

  func testSeenAppsParseAndMerge() {
    let reply = bytes(2) + bytes(13) + ascii("com.whatsapp2") + bytes(8) + ascii("WhatsApp")
      + bytes(19) + ascii("com.apple.MobileSMS") + bytes(0)
    let seen = Proto.parseSeenApps(reply)!
    XCTAssertEqual(seen, [SeenApp(appId: "com.whatsapp2", name: "WhatsApp"),
                          SeenApp(appId: "com.apple.MobileSMS", name: "")])
    XCTAssertEqual(Proto.parseSeenApps(bytes(0)), [])
    XCTAssertNil(Proto.parseSeenApps(Data()))
    XCTAssertNil(Proto.parseSeenApps(bytes(1, 5) + ascii("ab")))

    let merged = AppCatalogue.merge(seen: seen + [SeenApp(appId: "com.example.zed", name: "aardvark")])
    let ids = merged.map { $0.appId }
    XCTAssertEqual(Set(ids).count, ids.count, "deduped")
    XCTAssertEqual(merged.first?.label, "aardvark") // sorted by label, case-insensitive
    XCTAssertEqual(merged.first { $0.appId == "com.apple.MobileSMS" }?.label, "Messages") // empty name keeps catalogue label
    XCTAssertTrue(ids.contains("com.whatsapp2"))
    XCTAssertTrue(ids.contains("net.whatsapp.WhatsApp"))
    let labels = merged.map { $0.label.lowercased() }
    XCTAssertEqual(labels, labels.sorted())
  }

  func testNotifConfigMapRoundTrip() {
    let c = NotifConfig(enabled: false, cardSecs: 9, defaultMood: 3, quietStartMin: 60, quietEndMin: 120,
                        callsInQuiet: true, followDnd: false, calls: false,
                        rules: [NotifRule(appId: "com.apple.MobileSMS", mood: 0xFF, show: true)])
    XCTAssertEqual(NotifConfig.fromMap(c.toMap()), c)
    // What Flutter's standard codec hands over: NSNumber for ints and bools.
    let fromDart: [String: Any] = [
      "enabled": NSNumber(value: false), "cardSecs": NSNumber(value: 9), "defaultMood": NSNumber(value: 3),
      "quietStartMin": NSNumber(value: 60), "quietEndMin": NSNumber(value: 120),
      "callsInQuiet": NSNumber(value: true), "followDnd": NSNumber(value: false), "calls": NSNumber(value: false),
      "rules": [["appId": "com.apple.MobileSMS", "mood": NSNumber(value: 0xFF), "show": NSNumber(value: true)]],
    ]
    XCTAssertEqual(NotifConfig.fromMap(fromDart), c)
    XCTAssertNil(NotifConfig.fromMap(["enabled": true]))
    let json = try! JSONEncoder().encode(c)
    XCTAssertEqual(try! JSONDecoder().decode(NotifConfig.self, from: json), c)
  }

  // MARK: MessageQueue

  private final class FakeClock {
    var timers = [(ms: Int, run: () -> Void, cancelled: Bool)]()
    func sched(_ ms: Int, _ run: @escaping () -> Void) -> () -> Void {
      let i = timers.count
      timers.append((ms: ms, run: run, cancelled: false))
      return { [weak self] in self?.timers[i].cancelled = true }
    }
    func fireAll() {
      let t = timers
      for (i, x) in t.enumerated() where !x.cancelled {
        timers[i].cancelled = true
        x.run()
      }
    }
  }

  func testQueueStopAndWait() {
    let clock = FakeClock()
    var sent = [Data]()
    let q = MessageQueue(sched: clock.sched, transport: { f, done in sent.append(f); done(true) })
    var results = [Result<Data, LinkError>]()

    q.enqueue(0x02, Data()) { results.append($0) }
    XCTAssertEqual(results.count, 1, "closed queue refuses")
    if case .failure(let e) = results[0] { XCTAssertEqual(e.code, "NOT_CONNECTED") } else { XCTFail() }
    results.removeAll()

    q.open()
    q.enqueue(0x02, Data()) { results.append($0) }
    q.enqueue(0x10, Data()) { results.append($0) }
    XCTAssertEqual(sent.count, 1, "one in flight")
    XCTAssertEqual([UInt8](sent[0]), [0x02, 1, 0, 0])

    q.onMessage(Reassembler.Msg(op: 0x90, seq: 1, payload: bytes(0))) // wrong op: ignored
    q.onMessage(Reassembler.Msg(op: 0x82, seq: 9, payload: bytes(0))) // wrong seq: ignored
    XCTAssertTrue(results.isEmpty)
    q.onMessage(Reassembler.Msg(op: 0x82, seq: 1, payload: bytes(0, 3, 0x05, 0x00, 1, 0)))
    XCTAssertEqual(try? results[0].get(), bytes(3, 0x05, 0x00, 1, 0))
    XCTAssertEqual(sent.count, 2)
    XCTAssertEqual([UInt8](sent[1]), [0x10, 2, 0, 0])

    q.onMessage(Reassembler.Msg(op: 0x90, seq: 2, payload: bytes(Proto.stBadSlot)))
    if case .failure(let e) = results[1] {
      XCTAssertEqual(e.code, "DEVICE_ERROR")
      XCTAssertEqual(e.deviceStatus, Proto.stBadSlot)
    } else { XCTFail() }
  }

  func testQueueTimeoutAndClose() {
    let clock = FakeClock()
    var sent = 0
    let q = MessageQueue(sched: clock.sched, transport: { _, done in sent += 1; done(true) })
    q.open()
    var codes = [String]()
    q.enqueue(0x02, Data()) { if case .failure(let e) = $0 { codes.append(e.code) } }
    q.enqueue(0x03, Data()) { if case .failure(let e) = $0 { codes.append(e.code) } }
    q.enqueue(0x10, Data()) { if case .failure(let e) = $0 { codes.append(e.code) } }
    clock.fireAll() // first times out, second goes out
    XCTAssertEqual(codes, ["TIMEOUT"])
    XCTAssertEqual(sent, 2)
    q.close()
    XCTAssertEqual(codes, ["TIMEOUT", "NOT_CONNECTED", "NOT_CONNECTED"])
    XCTAssertFalse(q.busy)
    XCTAssertFalse(q.isOpen)
  }

  func testQueueWriteFailure() {
    let q = MessageQueue(sched: { _, _ in {} }, transport: { _, done in done(false) })
    q.open()
    var code: String?
    q.enqueue(0x02, Data()) { if case .failure(let e) = $0 { code = e.code } }
    XCTAssertEqual(code, "NOT_CONNECTED")
    XCTAssertFalse(q.busy)
  }
}
