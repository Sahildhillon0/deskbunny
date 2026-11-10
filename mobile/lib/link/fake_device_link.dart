// In-memory DeviceLink: emulates the bunny's slot store, the status
// transitions and the op replies (ARCHITECTURE.md "BLE GATT") so the whole UI
// runs without hardware. Select it with --dart-define=FAKE_LINK=true.
//
// It works on raw message bytes, like the real device, so it exercises the
// codecs in lib/protocol when used with the real Bunny.
import 'dart:async';
import 'dart:typed_data';

import 'device_link.dart';

class FakeDeviceLink implements DeviceLink {
  /// Artificial latency of every call (pairing takes 4 steps of it).
  final Duration delay;

  FakeDeviceLink({
    this.delay = const Duration(milliseconds: 250),
    bool paired = false,
    this.chip = 'esp32c3',
    this.fwVersion = '0.2.0',
    this.notificationAccess = false,
  }) {
    _slots[0] = defaultFaceBlob();
    if (paired) {
      _deviceId = 'A1B2C3';
      _status = LinkStatus(LinkState.connected, deviceId: _deviceId, fwVersion: fwVersion, chip: chip);
    }
  }

  final String chip;
  final String fwVersion;
  bool notificationAccess;

  // Device side.
  final Map<int, Uint8List> _slots = {}; // slot -> face[64] + art
  int activeSlot = 0;
  bool dnd = false;
  bool notifEnabled = true;
  String? _deviceId;

  // Phone side.
  NotifConfig _config = const NotifConfig();
  int clockStyle = 0; // phone side: the saved choice (0 = the face)
  LinkStatus _status = const LinkStatus(LinkState.unpaired);
  final _controller = StreamController<LinkStatus>.broadcast();

  /// Every request (op, payload) in order, for tests.
  final List<(int, Uint8List)> requests = [];
  int testNotifications = 0;
  int batteryRequests = 0;

  /// Next pair() throws this code instead of pairing (tests and demos).
  String? failNextPair;

  Map<int, Uint8List> get slotBlobs => Map.unmodifiable(_slots);

  Future<void> _wait() => delay == Duration.zero ? Future<void>.value() : Future<void>.delayed(delay);

  void _emit(LinkStatus s) {
    _status = s;
    _controller.add(s);
  }

  /// Simulate the bunny going out of range / coming back.
  void simulateDisconnect() => _emit(LinkStatus(LinkState.disconnected, deviceId: _deviceId));
  void simulateReconnect() =>
      _emit(LinkStatus(LinkState.connected, deviceId: _deviceId, fwVersion: fwVersion, chip: chip));

  @override
  Stream<LinkStatus> get status async* {
    yield _status;
    yield* _controller.stream;
  }

  @override
  Future<LinkStatus> currentStatus() async => _status;

  static final _qr = RegExp(r'^NIXXA1:([0-9A-F]{6}):([0-9A-F]{32})$');

  @override
  Future<LinkStatus> pair(String qrText) async {
    final m = _qr.firstMatch(qrText.trim().toUpperCase());
    if (m == null) throw const LinkException('BAD_QR', 'Not a nixxa QR code');
    final id = m.group(1)!;
    _emit(LinkStatus(LinkState.connecting, deviceId: id));
    await _wait();
    final fail = failNextPair;
    failNextPair = null;
    if (fail != null) {
      _emit(const LinkStatus(LinkState.unpaired));
      throw LinkException(fail, 'Simulated failure');
    }
    await _wait();
    _deviceId = id;
    final s = LinkStatus(LinkState.connected, deviceId: id, fwVersion: fwVersion, chip: chip);
    _emit(s);
    return s;
  }

  @override
  Future<void> forget() async {
    await _wait();
    _deviceId = null;
    _emit(const LinkStatus(LinkState.unpaired));
  }

  LinkException _devErr(int status) => LinkException('DEVICE_ERROR', 'Device status $status', deviceStatus: status);

  @override
  Future<Uint8List> request(int op, Uint8List payload) async {
    requests.add((op, Uint8List.fromList(payload)));
    await _wait();
    if (_status.state != LinkState.connected) throw const LinkException('NOT_CONNECTED', 'Not connected');
    final out = BytesBuilder();
    switch (op) {
      case 0x02: // GET_STATE
        var mask = 0;
        for (final s in _slots.keys) {
          mask |= 1 << s;
        }
        out
          ..addByte(activeSlot)
          ..add([mask & 0xFF, mask >> 8])
          ..addByte(notifEnabled ? 1 : 0)
          ..addByte(dnd ? 1 : 0);
      case 0x03: // SET_TIME
        break;
      case 0x10: // LIST_SLOTS
        final used = _slots.keys.toList()..sort();
        out.addByte(used.length);
        for (final s in used) {
          out.addByte(s);
          var name = _slots[s]!.sublist(10, 22);
          if (name.every((b) => b == 0)) {
            name = Uint8List(12)..setAll(0, 'SLOT ${s.toString().padLeft(2, '0')}'.codeUnits);
          }
          out.add(name);
        }
      case 0x11: // PUT_SLOT
        if (payload.length < 67) throw _devErr(2);
        final slot = payload[0];
        if (slot >= 16) throw _devErr(3);
        final face = payload.sublist(1, 65);
        if (crc16(face, 62) != (face[62] | face[63] << 8)) throw _devErr(2);
        final artLen = payload[65] | payload[66] << 8;
        if (payload.length != 67 + artLen) throw _devErr(2);
        if (artLen > 12285) throw _devErr(4);
        _slots[slot] = Uint8List.fromList(payload.sublist(1));
      case 0x12: // GET_SLOT
        final blob = payload.isEmpty ? null : _slots[payload[0]];
        if (blob == null) throw _devErr(3);
        out
          ..add(blob.sublist(0, 64))
          ..add([(blob.length - 64) & 0xFF, (blob.length - 64) >> 8])
          ..add(blob.sublist(64));
      case 0x13: // DELETE_SLOT
        if (payload.isEmpty || payload[0] >= 16 || payload[0] == activeSlot) throw _devErr(3);
        _slots.remove(payload[0]);
      case 0x14: // ACTIVATE
        if (payload.isEmpty || !_slots.containsKey(payload[0])) throw _devErr(3);
        activeSlot = payload[0];
      case 0x20: // SET_NOTIF_CONFIG
        if (payload.isNotEmpty) notifEnabled = payload[0] != 0;
      case 0x21 || 0x22: // NOTIFY, DISMISS
        break;
      case 0x23: // SET_DND
        dnd = payload.isNotEmpty && payload[0] != 0;
      case 0x7F: // UNPAIR
        break;
      default:
        throw _devErr(2);
    }
    return out.toBytes();
  }

  @override
  Future<NotifConfig> getNotifConfig() async {
    await _wait();
    return _config;
  }

  @override
  Future<void> setNotifConfig(NotifConfig config) async {
    await _wait();
    _config = config;
    notifEnabled = config.enabled;
  }

  @override
  Future<int> getClock() async {
    await _wait();
    return clockStyle;
  }

  @override
  Future<void> setClock(int style) async {
    await _wait();
    clockStyle = style;
  }

  @override
  Future<bool> hasNotificationAccess() async => notificationAccess;

  @override
  Future<void> openNotificationAccessSettings() async {
    await _wait();
    notificationAccess = true;
  }

  @override
  Future<void> requestBatteryExemption() async => batteryRequests++;

  @override
  Future<List<AppInfo>> listApps() async {
    await _wait();
    return const [
      AppInfo('com.google.android.gm', 'Gmail', null),
      AppInfo('com.google.android.calendar', 'Calendar', null),
      AppInfo('com.discord', 'Discord', null),
      AppInfo('org.telegram.messenger', 'Telegram', null),
      AppInfo('com.slack', 'Slack', null),
      AppInfo('com.whatsapp', 'WhatsApp', null),
    ];
  }

  @override
  Future<void> sendTestNotification() async {
    await _wait();
    if (_status.state != LinkState.connected) throw const LinkException('NOT_CONNECTED', 'Not connected');
    testNotifications++;
  }

  // ---------------------------------------------------------------- blob helpers

  /// CRC-16/CCITT-FALSE over the first [len] bytes.
  static int crc16(List<int> data, int len) {
    var crc = 0xFFFF;
    for (var i = 0; i < len; i++) {
      crc ^= data[i] << 8;
      for (var b = 0; b < 8; b++) {
        crc = (crc & 0x8000) != 0 ? ((crc << 1) ^ 0x1021) & 0xFFFF : (crc << 1) & 0xFFFF;
      }
    }
    return crc;
  }

  /// db_face_default(): neutral, blink + look_around + breathe, name DESKBUNNY.
  static Uint8List defaultFaceBlob({String name = 'DESKBUNNY'}) {
    final b = Uint8List(64);
    b.setAll(0, 'DBFACE01'.codeUnits);
    b[8] = 1; // version
    b[9] = 0; // mood
    final n = name.codeUnits.take(12).toList();
    b.setAll(10, n);
    b.setAll(22, [36, 36, 8, 16, 0, 0x07, 20, 60, 15, 50, 18, 8, 5, 0xCF, 0, 0, 0, 10, 0, 1]);
    final crc = crc16(b, 62);
    b[62] = crc & 0xFF;
    b[63] = crc >> 8;
    return b;
  }
}
