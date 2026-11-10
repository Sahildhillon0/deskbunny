// The phone <-> bunny link. On Android this is a native Kotlin foreground
// service reached over platform channels (see ARCHITECTURE.md "Mobile app").
// This file is the contract; do not change it without updating the doc.
import 'dart:typed_data';

enum LinkState { bluetoothOff, unpaired, disconnected, connecting, connected }

class LinkStatus {
  final LinkState state;
  final String? deviceId; // 6 hex, when paired
  final String? fwVersion; // from INFO, when connected
  final String? chip; // "esp32" | "esp32c3" | "esp32s3", when connected
  const LinkStatus(this.state, {this.deviceId, this.fwVersion, this.chip});

  static LinkStatus fromMap(Map<Object?, Object?> m) => LinkStatus(
        LinkState.values.byName(m['state'] as String),
        deviceId: m['deviceId'] as String?,
        fwVersion: m['fwVersion'] as String?,
        chip: m['chip'] as String?,
      );
  Map<String, Object?> toMap() =>
      {'state': state.name, 'deviceId': deviceId, 'fwVersion': fwVersion, 'chip': chip};
}

class NotifRule {
  final String appId; // Android package name
  final int mood; // Mood index, 0xFF = default mood
  final bool show;
  const NotifRule(this.appId, {this.mood = 0xFF, this.show = true});
  Map<String, Object?> toMap() => {'appId': appId, 'mood': mood, 'show': show};
  static NotifRule fromMap(Map<Object?, Object?> m) =>
      NotifRule(m['appId'] as String, mood: m['mood'] as int, show: m['show'] as bool);
}

class NotifConfig {
  final bool enabled;
  final int cardSecs; // 2..30
  final int defaultMood; // Mood index
  final int quietStartMin, quietEndMin; // minutes after midnight; equal = off
  final bool callsInQuiet;
  final bool followDnd; // mirror the phone's Do Not Disturb (SET_DND)
  final bool calls; // forward incoming calls
  final List<NotifRule> rules; // apps not listed are NOT shown
  const NotifConfig({
    this.enabled = true, this.cardSecs = 6, this.defaultMood = 5,
    this.quietStartMin = 0, this.quietEndMin = 0, this.callsInQuiet = false,
    this.followDnd = true, this.calls = true, this.rules = const [],
  });
  Map<String, Object?> toMap() => {
        'enabled': enabled, 'cardSecs': cardSecs, 'defaultMood': defaultMood,
        'quietStartMin': quietStartMin, 'quietEndMin': quietEndMin,
        'callsInQuiet': callsInQuiet, 'followDnd': followDnd, 'calls': calls,
        'rules': rules.map((r) => r.toMap()).toList(),
      };
  static NotifConfig fromMap(Map<Object?, Object?> m) => NotifConfig(
        enabled: m['enabled'] as bool, cardSecs: m['cardSecs'] as int,
        defaultMood: m['defaultMood'] as int, quietStartMin: m['quietStartMin'] as int,
        quietEndMin: m['quietEndMin'] as int, callsInQuiet: m['callsInQuiet'] as bool,
        followDnd: m['followDnd'] as bool, calls: m['calls'] as bool,
        rules: (m['rules'] as List).map((r) => NotifRule.fromMap(r as Map)).toList(),
      );
}

class AppInfo {
  final String appId;
  final String label;
  final Uint8List? iconPng; // 48x48 PNG for the picker
  const AppInfo(this.appId, this.label, this.iconPng);
}

/// Codes: BT_OFF, PERMISSION, NOT_FOUND, BOND_FAILED, AUTH_FAILED,
/// NOT_CONNECTED, TIMEOUT, DEVICE_ERROR (deviceStatus set), BAD_QR.
class LinkException implements Exception {
  final String code;
  final String message;
  final int? deviceStatus; // reply status byte for DEVICE_ERROR
  const LinkException(this.code, this.message, {this.deviceStatus});
  @override
  String toString() => 'LinkException($code): $message';
}

abstract class DeviceLink {
  Stream<LinkStatus> get status; // emits current status on listen
  Future<LinkStatus> currentStatus();

  /// Pair from the raw QR text: scan, connect, bond, MTU, INFO, AUTH, persist,
  /// start the background service. Throws LinkException.
  Future<LinkStatus> pair(String qrText);

  /// UNPAIR (if connected), forget the key and the bond, stop the service.
  Future<void> forget();

  /// Send one message (op + payload); returns the reply payload after the
  /// status byte. Non-zero status -> LinkException('DEVICE_ERROR').
  Future<Uint8List> request(int op, Uint8List payload);

  Future<NotifConfig> getNotifConfig();
  /// Persists, filters on the phone, and sends SET_NOTIF_CONFIG now and on
  /// every reconnect.
  Future<void> setNotifConfig(NotifConfig config);

  /// The clock style chosen on this phone: 0 = the face, 1..20 = a clock.
  Future<int> getClock();
  /// Persists, and sends SET_CLOCK now (when connected) and on every reconnect.
  /// Firmware without clocks ignores it. Works offline: the choice is kept.
  Future<void> setClock(int style);

  Future<bool> hasNotificationAccess();
  Future<void> openNotificationAccessSettings();
  Future<void> requestBatteryExemption();
  Future<List<AppInfo>> listApps();
  Future<void> sendTestNotification();
}
