import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/link/device_link.dart';
import 'package:nixxa/model/drawing.dart';
import 'package:nixxa/model/face.dart';
import 'package:nixxa/model/presets.dart';
import 'package:nixxa/protocol/blob.dart';
import 'package:nixxa/protocol/bunny.dart';
import 'package:nixxa/protocol/notif.dart';

/// Records requests and answers from a queue.
class _Link implements DeviceLink {
  final sent = <(int, Uint8List)>[];
  final replies = <Uint8List>[];

  @override
  Future<Uint8List> request(int op, Uint8List payload) async {
    sent.add((op, Uint8List.fromList(payload)));
    return replies.isEmpty ? Uint8List(0) : replies.removeAt(0);
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Uint8List _bytes(List<int> b) => Uint8List.fromList(b);

void main() {
  late _Link link;
  late Bunny bunny;
  setUp(() {
    link = _Link();
    bunny = Bunny(link);
  });

  test('GET_STATE', () async {
    link.replies.add(_bytes([3, 0x09, 0x80, 1, 0]));
    final s = await bunny.getState();
    expect(link.sent.single.$1, 0x02);
    expect(link.sent.single.$2, isEmpty);
    expect([s.activeSlot, s.usedMask, s.notifEnabled, s.dnd], [3, 0x8009, true, false]);
    expect(s.isUsed(15), isTrue);
    expect(s.isUsed(1), isFalse);
  });

  test('LIST_SLOTS', () async {
    final r = BytesBuilder()
      ..addByte(2)
      ..addByte(0)
      ..add('DESKBUNNY'.codeUnits)
      ..add([0, 0, 0])
      ..addByte(5)
      ..add('SLOT 05     '.codeUnits);
    link.replies.add(r.takeBytes());
    final slots = await bunny.listSlots();
    expect(link.sent.single.$1, 0x10);
    expect(slots.map((s) => (s.slot, s.name)), [(0, 'DESKBUNNY'), (5, 'SLOT 05     ')]);
  });

  test('PUT_SLOT without and with a drawing', () async {
    await bunny.putSlot(4, presets[2], null);
    var (op, p) = link.sent.last;
    expect(op, 0x11);
    expect(p.length, 67);
    expect(p[0], 4);
    expect(p.sublist(1, 65), encodeFace(presets[2]));
    expect([p[65], p[66]], [0, 0]);

    final frame = Uint8List(1024)..[7] = 0xFF;
    final d = Drawing(mode: DrawingMode.over, frameMs: 300, frames: [frame]);
    await bunny.putSlot(15, presets[0], d);
    (op, p) = link.sent.last;
    final art = encodeDrawing(d);
    expect(p[0], 15);
    expect(p[65] | (p[66] << 8), art.length);
    expect(p.sublist(67), art);

    // An empty drawing is "no drawing".
    await bunny.putSlot(1, presets[0], const Drawing(mode: DrawingMode.replace, frames: []));
    expect(link.sent.last.$2.length, 67);
  });

  test('PUT_SLOT refuses bad slots and oversized drawings', () {
    expect(() => bunny.putSlot(16, presets[0], null), throwsArgumentError);
    expect(() => bunny.putSlot(-1, presets[0], null), throwsArgumentError);
    // PackBits' worst case: a run of 2 then a lone byte costs 4 bytes per 3.
    Uint8List worst(int k) => Uint8List.fromList(List.generate(1024, (i) => i % 3 == 2 ? k : 0xF0));
    final heavy = Drawing(mode: DrawingMode.over, frames: List.generate(10, worst));
    expect(drawingSize(heavy), greaterThan(maxArtBytes));
    expect(() => bunny.putSlot(0, presets[0], heavy), throwsArgumentError);
  });

  test('GET_SLOT', () async {
    final d = Drawing(mode: DrawingMode.replace, frameMs: 120, frames: [Uint8List(1024)..[0] = 1]);
    final art = encodeDrawing(d);
    link.replies.add(_bytes([...encodeFace(presets[6]), art.length & 0xFF, art.length >> 8, ...art]));
    final (face, drawing) = await bunny.getSlot(9);
    expect(link.sent.single.$1, 0x12);
    expect(link.sent.single.$2, [9]);
    expect(face, presets[6]);
    expect(drawing, d);

    link.replies.add(_bytes([...encodeFace(presets[1]), 0, 0]));
    final (f2, d2) = await bunny.getSlot(0);
    expect(f2, presets[1]);
    expect(d2, isNull);

    link.replies.add(_bytes([1, 2, 3]));
    expect(() => bunny.getSlot(0), throwsFormatException);
  });

  test('DELETE_SLOT / ACTIVATE', () async {
    await bunny.deleteSlot(7);
    await bunny.activate(2);
    expect(link.sent.map((s) => s.$1), [0x13, 0x14]);
    expect(link.sent.map((s) => s.$2.single), [7, 2]);
  });

  test('SET_TIME', () async {
    final t = DateTime.utc(2026, 11, 5, 12, 0, 1);
    await bunny.setTime(t);
    final (op, p) = link.sent.single;
    expect(op, 0x03);
    final bd = ByteData.sublistView(p);
    expect(p.length, 6);
    expect(bd.getUint32(0, Endian.little), t.millisecondsSinceEpoch ~/ 1000);
    expect(bd.getInt16(4, Endian.little), 0);
    final local = DateTime.now();
    expect(ByteData.sublistView(Bunny.encodeSetTime(local)).getInt16(4, Endian.little),
        local.timeZoneOffset.inMinutes);
  });

  test('SET_CLOCK payload: one byte, 0 = the face, 1..20 = a clock', () {
    expect(BunnyOp.setClock, 0x25);
    expect(Bunny.encodeSetClock(0), [0]);
    expect(Bunny.encodeSetClock(1), [1]);
    expect(Bunny.encodeSetClock(20), [20]);
    expect(() => Bunny.encodeSetClock(21), throwsArgumentError);
    expect(() => Bunny.encodeSetClock(-1), throwsArgumentError);
  });

  test('SET_NOTIF_CONFIG payload', () {
    final p = encodeNotifConfig(const NotifConfig(
      enabled: true,
      cardSecs: 99,
      defaultMood: 5,
      quietStartMin: 1320,
      quietEndMin: 420,
      callsInQuiet: true,
      rules: [
        NotifRule('com.whatsapp', mood: 1),
        NotifRule('org.x', show: false),
        NotifRule('bad.é'),
      ],
    ));
    expect(p, [
      1, 30, 5, 0x28, 0x05, 0xA4, 0x01, 1, 2,
      12, ...'com.whatsapp'.codeUnits, 1, 1,
      5, ...'org.x'.codeUnits, 0xFF, 0,
      1, // ext: bit0 = calls
    ]);
    final many = encodeNotifConfig(NotifConfig(
        enabled: false, calls: false, rules: List.generate(40, (i) => NotifRule('a$i'))));
    expect(many[0], 0);
    expect(many[8], 32);
    // 32 rules, then exactly one ext byte (calls off).
    var pos = 9;
    for (var i = 0; i < 32; i++) {
      pos += 1 + many[pos] + 2;
    }
    expect(many.length, pos + 1);
    expect(many.last, 0);
  });

  test('Mood indices used by NotifConfig match the blob enum', () {
    expect(Mood.surprised.index, 5);
  });
}
