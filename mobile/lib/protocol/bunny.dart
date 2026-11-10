// High-level device API over DeviceLink.request (ops 0x02..0x14), payloads
// per the GATT op table in ARCHITECTURE.md "BLE GATT".
import 'dart:typed_data';

import '../link/device_link.dart';
import '../model/drawing.dart';
import '../model/face.dart';
import 'blob.dart';

class DeviceState {
  final int activeSlot;
  final int usedMask; // bit n = slot n used
  final bool notifEnabled;
  final bool dnd;
  const DeviceState(this.activeSlot, this.usedMask, this.notifEnabled, this.dnd);

  bool isUsed(int slot) => usedMask & (1 << slot) != 0;
}

class SlotInfo {
  final int slot;
  final String name;
  const SlotInfo(this.slot, this.name);
}

/// GATT op codes.
abstract final class BunnyOp {
  static const int auth = 0x01;
  static const int getState = 0x02;
  static const int setTime = 0x03;
  static const int listSlots = 0x10;
  static const int putSlot = 0x11;
  static const int getSlot = 0x12;
  static const int deleteSlot = 0x13;
  static const int activate = 0x14;
  static const int setNotifConfig = 0x20;
  static const int notify = 0x21;
  static const int dismiss = 0x22;
  static const int setDnd = 0x23;
  static const int setClock = 0x25;
  static const int unpair = 0x7F;
}

class Bunny {
  final DeviceLink link;
  Bunny(this.link);

  static const int slots = 16;

  /// Clock styles on the bunny, numbered 1..clockStyles; 0 means "the face".
  static const int clockStyles = 20;

  Future<DeviceState> getState() async {
    final r = await link.request(BunnyOp.getState, Uint8List(0));
    _need(r, 5, 'GET_STATE');
    return DeviceState(r[0], r[1] | (r[2] << 8), r[3] != 0, r[4] != 0);
  }

  Future<List<SlotInfo>> listSlots() async {
    final r = await link.request(BunnyOp.listSlots, Uint8List(0));
    _need(r, 1, 'LIST_SLOTS');
    final n = r[0];
    _need(r, 1 + n * 13, 'LIST_SLOTS');
    return [
      for (var i = 0; i < n; i++) SlotInfo(r[1 + i * 13], _ascii(r, 2 + i * 13, 12)),
    ];
  }

  /// Throws ArgumentError for a bad slot or a drawing over [maxArtBytes].
  /// A null drawing, or one with no frames, is sent as artLen 0.
  Future<void> putSlot(int slot, FaceDef face, Drawing? drawing) async {
    await link.request(BunnyOp.putSlot, encodePutSlot(slot, face, drawing));
  }

  Future<(FaceDef, Drawing?)> getSlot(int slot) async {
    final r = await link.request(BunnyOp.getSlot, Uint8List.fromList([_slot(slot)]));
    return decodeGetSlotReply(r);
  }

  Future<void> deleteSlot(int slot) async {
    await link.request(BunnyOp.deleteSlot, Uint8List.fromList([_slot(slot)]));
  }

  Future<void> activate(int slot) async {
    await link.request(BunnyOp.activate, Uint8List.fromList([_slot(slot)]));
  }

  /// SET_TIME: unix seconds + the zone offset of [time] (pass DateTime.now()).
  Future<void> setTime(DateTime time) async {
    await link.request(BunnyOp.setTime, encodeSetTime(time));
  }

  // ---- pure payload builders/parsers (also used by tests) ----

  static Uint8List encodePutSlot(int slot, FaceDef face, Drawing? drawing) {
    final art = drawing?.sanitized() == null ? Uint8List(0) : encodeDrawing(drawing!);
    if (art.length > maxArtBytes) {
      throw ArgumentError('This drawing needs ${art.length} bytes but the bunny holds $maxArtBytes. '
          'Use fewer frames or simpler pictures.');
    }
    final b = Uint8List(1 + faceBlobSize + 2 + art.length);
    b[0] = _slot(slot);
    b.setRange(1, 1 + faceBlobSize, encodeFace(face));
    b[65] = art.length & 0xFF;
    b[66] = art.length >> 8;
    b.setRange(67, b.length, art);
    return b;
  }

  static (FaceDef, Drawing?) decodeGetSlotReply(Uint8List r) {
    _need(r, faceBlobSize + 2, 'GET_SLOT');
    final face = decodeFace(Uint8List.sublistView(r, 0, faceBlobSize));
    final artLen = r[64] | (r[65] << 8);
    _need(r, faceBlobSize + 2 + artLen, 'GET_SLOT');
    final drawing = artLen == 0 ? null : decodeDrawing(Uint8List.sublistView(r, 66, 66 + artLen));
    return (face, drawing);
  }

  /// SET_CLOCK: 0 = the face, 1..20 = that clock. Throws ArgumentError otherwise.
  static Uint8List encodeSetClock(int style) {
    if (style < 0 || style > clockStyles) {
      throw ArgumentError.value(style, 'style', 'must be 0..$clockStyles');
    }
    return Uint8List.fromList([style]);
  }

  static Uint8List encodeSetTime(DateTime time) {
    final secs = time.millisecondsSinceEpoch ~/ 1000;
    final tz = time.timeZoneOffset.inMinutes;
    final b = ByteData(6)
      ..setUint32(0, secs & 0xFFFFFFFF, Endian.little)
      ..setInt16(4, tz, Endian.little);
    return b.buffer.asUint8List();
  }

  static int _slot(int slot) {
    if (slot < 0 || slot >= slots) throw ArgumentError.value(slot, 'slot', 'must be 0..${slots - 1}');
    return slot;
  }

  static void _need(Uint8List r, int n, String op) {
    if (r.length < n) throw FormatException('$op reply too short (${r.length} < $n bytes).');
  }

  static String _ascii(Uint8List r, int off, int len) {
    final s = StringBuffer();
    for (var i = off; i < off + len && r[i] != 0; i++) {
      s.writeCharCode(r[i] >= 0x20 && r[i] < 0x7F ? r[i] : 0x3F);
    }
    return s.toString();
  }
}
