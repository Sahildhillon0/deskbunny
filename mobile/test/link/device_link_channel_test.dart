import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/link/device_link.dart';
import 'package:nixxa/link/device_link_channel.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const methods = MethodChannel('nixxa/link');
  const events = EventChannel('nixxa/link/status');
  final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  late List<MethodCall> calls;
  late Object? Function(MethodCall) handler;

  setUp(() {
    calls = [];
    handler = (_) => null;
    messenger.setMockMethodCallHandler(methods, (call) async {
      calls.add(call);
      return handler(call);
    });
  });

  tearDown(() {
    messenger.setMockMethodCallHandler(methods, null);
    messenger.setMockStreamHandler(events, null);
  });

  final connected = {'state': 'connected', 'deviceId': 'A1B2C3', 'fwVersion': '0.2.0', 'chip': 'esp32c3'};

  test('status and pair map LinkStatus', () async {
    final link = DeviceLinkChannel();
    handler = (c) => connected;
    final st = await link.currentStatus();
    expect(st.state, LinkState.connected);
    expect(st.chip, 'esp32c3');
    final p = await link.pair('NIXXA1:A1B2C3:00112233445566778899AABBCCDDEEFF');
    expect(p.deviceId, 'A1B2C3');
    expect(calls.map((c) => c.method), ['status', 'pair']);
    expect(calls[1].arguments, {'qr': 'NIXXA1:A1B2C3:00112233445566778899AABBCCDDEEFF'});
  });

  test('request passes op and payload, returns bytes', () async {
    final link = DeviceLinkChannel();
    handler = (c) => Uint8List.fromList([5, 1, 0]);
    final r = await link.request(0x02, Uint8List(0));
    expect(r, [5, 1, 0]);
    expect(calls.single.method, 'request');
    expect(calls.single.arguments, {'op': 2, 'payload': Uint8List(0)});
  });

  test('PlatformException becomes LinkException with deviceStatus', () async {
    final link = DeviceLinkChannel();
    handler = (c) => throw PlatformException(code: 'DEVICE_ERROR', message: 'Device status BAD_SLOT', details: 3);
    await expectLater(
      link.request(0x13, Uint8List.fromList([4])),
      throwsA(isA<LinkException>()
          .having((e) => e.code, 'code', 'DEVICE_ERROR')
          .having((e) => e.deviceStatus, 'deviceStatus', 3)),
    );
    handler = (c) => throw PlatformException(code: 'NOT_CONNECTED', message: 'Not connected');
    await expectLater(
      link.sendTestNotification(),
      throwsA(isA<LinkException>()
          .having((e) => e.code, 'code', 'NOT_CONNECTED')
          .having((e) => e.deviceStatus, 'deviceStatus', null)),
    );
  });

  test('notif config round trip through the channel maps', () async {
    final link = DeviceLinkChannel();
    const cfg = NotifConfig(
      cardSecs: 9, quietStartMin: 1320, quietEndMin: 420, followDnd: false,
      rules: [NotifRule('com.whatsapp', mood: 6), NotifRule('com.slack', show: false)],
    );
    Object? stored;
    handler = (c) {
      if (c.method == 'setNotifConfig') stored = c.arguments;
      if (c.method == 'getNotifConfig') return stored;
      return null;
    };
    await link.setNotifConfig(cfg);
    final back = await link.getNotifConfig();
    expect(back.toMap(), cfg.toMap());
  });

  test('clock style round trip through the channel', () async {
    final link = DeviceLinkChannel();
    int? stored;
    handler = (c) {
      if (c.method == 'setClock') stored = (c.arguments as Map)['style'] as int;
      if (c.method == 'getClock') return stored ?? 0;
      return null;
    };
    expect(await link.getClock(), 0);
    await link.setClock(7);
    expect(calls.last.method, 'setClock');
    expect(calls.last.arguments, {'style': 7});
    expect(await link.getClock(), 7);
  });

  test('listApps and access helpers', () async {
    final link = DeviceLinkChannel();
    handler = (c) => switch (c.method) {
          'listApps' => [
              {'appId': 'com.a', 'label': 'A', 'iconPng': Uint8List.fromList([137, 80])},
              {'appId': 'com.b', 'label': 'B', 'iconPng': null},
            ],
          'hasNotificationAccess' => true,
          _ => null,
        };
    final apps = await link.listApps();
    expect(apps.map((a) => a.appId), ['com.a', 'com.b']);
    expect(apps[0].iconPng, [137, 80]);
    expect(apps[1].iconPng, isNull);
    expect(await link.hasNotificationAccess(), isTrue);
    await link.openNotificationAccessSettings();
    await link.requestBatteryExemption();
    await link.forget();
    expect(calls.map((c) => c.method),
        ['listApps', 'hasNotificationAccess', 'openNotificationAccessSettings', 'requestBatteryExemption', 'forget']);
  });

  test('status stream decodes events', () async {
    messenger.setMockStreamHandler(
      events,
      MockStreamHandler.inline(onListen: (args, sink) {
        sink.success({'state': 'disconnected', 'deviceId': 'A1B2C3', 'fwVersion': null, 'chip': null});
        sink.success(connected);
      }),
    );
    final link = DeviceLinkChannel();
    final got = await link.status.take(2).toList();
    expect(got.map((s) => s.state), [LinkState.disconnected, LinkState.connected]);
    expect(got[1].fwVersion, '0.2.0');
  });
}
