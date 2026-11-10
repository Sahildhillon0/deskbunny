// SET_NOTIF_CONFIG (op 0x20) payload, ARCHITECTURE.md "BLE GATT".
// Android (Kotlin) and iOS (Swift) build their own; this mirrors them for tests.
import 'dart:convert';
import 'dart:typed_data';

import '../link/device_link.dart';

const int notifRulesMax = 32;
const int notifAppIdMax = 64;

/// `enabled u8, cardSecs u8 (2..30), defaultMood u8, quietStartMin u16,
/// quietEndMin u16, callsInQuiet u8, ruleCount u8 (<= 32)`, then per rule
/// `idLen u8, appId, mood u8, flags u8 (bit0 show)`, then the trailing
/// `ext u8` (bit0 = forward calls, from `calls`).
/// Values are clamped (cardSecs 2..30, minutes 0..1439, moods 0..255); rules
/// whose appId is empty, non-ASCII or longer than 64 bytes are skipped, and
/// only the first 32 remaining rules are sent. `followDnd` is a phone-side
/// setting and is not part of the payload.
Uint8List encodeNotifConfig(NotifConfig c) {
  final out = BytesBuilder();
  int minutes(int m) => m.clamp(0, 1439);
  void u16(int v) => out.add([v & 0xFF, (v >> 8) & 0xFF]);
  out.addByte(c.enabled ? 1 : 0);
  out.addByte(c.cardSecs.clamp(2, 30));
  out.addByte(c.defaultMood.clamp(0, 255));
  u16(minutes(c.quietStartMin));
  u16(minutes(c.quietEndMin));
  out.addByte(c.callsInQuiet ? 1 : 0);
  final rules = c.rules
      .where((r) => r.appId.isNotEmpty && r.appId.length <= notifAppIdMax && r.appId.codeUnits.every((u) => u < 0x80))
      .take(notifRulesMax)
      .toList();
  out.addByte(rules.length);
  for (final r in rules) {
    final id = ascii.encode(r.appId);
    out.addByte(id.length);
    out.add(id);
    out.addByte(r.mood.clamp(0, 255));
    out.addByte(r.show ? 1 : 0);
  }
  out.addByte(c.calls ? 1 : 0); // ext
  return out.takeBytes();
}
