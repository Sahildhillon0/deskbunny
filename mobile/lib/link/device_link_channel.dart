// DeviceLink over platform channels to the native Kotlin link
// (mobile/android/.../link/Link.kt). Channel table: ARCHITECTURE.md
// "Mobile app" -> "Android link (native, Kotlin)".
import 'package:flutter/services.dart';

import 'device_link.dart';

class DeviceLinkChannel implements DeviceLink {
  DeviceLinkChannel({
    MethodChannel methods = const MethodChannel('nixxa/link'),
    EventChannel events = const EventChannel('nixxa/link/status'),
  })  : _m = methods,
        _status = events
            .receiveBroadcastStream()
            .map((e) => LinkStatus.fromMap(e as Map<Object?, Object?>));

  final MethodChannel _m;
  final Stream<LinkStatus> _status;

  /// The native side emits the current status to every new listener.
  @override
  Stream<LinkStatus> get status => _status;

  Future<T?> _call<T>(String method, [Object? args]) async {
    try {
      return await _m.invokeMethod<T>(method, args);
    } on PlatformException catch (e) {
      throw LinkException(
        e.code,
        e.message ?? e.code,
        deviceStatus: e.details is int ? e.details as int : null,
      );
    }
  }

  Map<Object?, Object?> _map(Object? v) =>
      (v as Map<Object?, Object?>?) ?? const <Object?, Object?>{};

  @override
  Future<LinkStatus> currentStatus() async =>
      LinkStatus.fromMap(_map(await _call<Map<Object?, Object?>>('status')));

  @override
  Future<LinkStatus> pair(String qrText) async =>
      LinkStatus.fromMap(_map(await _call<Map<Object?, Object?>>('pair', {'qr': qrText})));

  @override
  Future<void> forget() => _call<void>('forget');

  @override
  Future<Uint8List> request(int op, Uint8List payload) async =>
      await _call<Uint8List>('request', {'op': op, 'payload': payload}) ?? Uint8List(0);

  @override
  Future<NotifConfig> getNotifConfig() async =>
      NotifConfig.fromMap(_map(await _call<Map<Object?, Object?>>('getNotifConfig')));

  @override
  Future<void> setNotifConfig(NotifConfig config) => _call<void>('setNotifConfig', config.toMap());

  @override
  Future<int> getClock() async => await _call<int>('getClock') ?? 0;

  @override
  Future<void> setClock(int style) => _call<void>('setClock', {'style': style});

  @override
  Future<bool> hasNotificationAccess() async =>
      await _call<bool>('hasNotificationAccess') ?? false;

  @override
  Future<void> openNotificationAccessSettings() => _call<void>('openNotificationAccessSettings');

  @override
  Future<void> requestBatteryExemption() => _call<void>('requestBatteryExemption');

  @override
  Future<List<AppInfo>> listApps() async {
    final list = await _call<List<Object?>>('listApps') ?? const [];
    return [
      for (final a in list.cast<Map<Object?, Object?>>())
        AppInfo(a['appId'] as String, a['label'] as String, a['iconPng'] as Uint8List?),
    ];
  }

  @override
  Future<void> sendTestNotification() => _call<void>('sendTestNotification');
}
