// Everything the widgets remember, in one JSON document the app screens write
// and the background scheduler reads, plus what the scheduler reports back.
// Stored with SharedPreferencesAsync, which reads straight from disk, so the
// two Dart isolates (the app and the background engine) always agree.
import 'dart:convert';

import 'package:shared_preferences/shared_preferences.dart';

import 'widget_data.dart';

/// The widgets that live in a slot on the bunny.
abstract final class WidgetIds {
  static const weather = 'weather';
  static const air = 'air';
  static const battery = 'battery';
  static const event = 'event';
  static const counter = 'counter';
  static const status = 'status';
  static const nowPlaying = 'nowplaying';

  /// The order they take when the bunny rotates.
  static const rotating = [weather, air, battery, event, counter];

  /// Widgets whose picture comes from the internet (refreshed on the long timer).
  static const network = {weather, air};

  /// The slot content's name on the bunny (12 characters at most).
  static String faceName(String id) => switch (id) {
    weather => 'WEATHER',
    air => 'AIR QUALITY',
    battery => 'BATTERY',
    event => 'NEXT EVENT',
    counter => 'COUNTER',
    status => 'STATUS',
    nowPlaying => 'NOW PLAYING',
    _ => 'WIDGET',
  };
}

const List<int> refreshChoices = [15, 30, 60]; // minutes
const List<int> rotateChoices = [30, 60, 300]; // seconds (each switch is a flash write on the bunny)

class WidgetSettings {
  /// Keep the widgets in [rotation] updated and rotate through them.
  final bool autoOn;
  final int refreshMins, rotateSecs;

  /// Put the bunny's own face in the rotation too.
  final bool rotateHome;

  /// Show the song that is playing on the bunny, as it plays.
  final bool musicOn;

  /// Widget id -> the bunny slot its picture lives in (set when first sent).
  final Map<String, int> slots;
  final Set<String> rotation;

  final GeoPlace? place;
  final bool fahrenheit;

  final int counter;
  final String counterLabel;
  final String statusText;
  final bool statusDnd;

  const WidgetSettings({
    this.autoOn = false,
    this.refreshMins = 30,
    this.rotateSecs = 60,
    this.rotateHome = true,
    this.musicOn = false,
    this.slots = const {},
    this.rotation = const {},
    this.place,
    this.fahrenheit = false,
    this.counter = 0,
    this.counterLabel = 'STREAK',
    this.statusText = 'IN A MEETING',
    this.statusDnd = false,
  });

  WidgetSettings copyWith({
    bool? autoOn,
    int? refreshMins,
    int? rotateSecs,
    bool? rotateHome,
    bool? musicOn,
    Map<String, int>? slots,
    Set<String>? rotation,
    GeoPlace? place,
    bool? fahrenheit,
    int? counter,
    String? counterLabel,
    String? statusText,
    bool? statusDnd,
  }) => WidgetSettings(
    autoOn: autoOn ?? this.autoOn,
    refreshMins: refreshMins ?? this.refreshMins,
    rotateSecs: rotateSecs ?? this.rotateSecs,
    rotateHome: rotateHome ?? this.rotateHome,
    musicOn: musicOn ?? this.musicOn,
    slots: slots ?? this.slots,
    rotation: rotation ?? this.rotation,
    place: place ?? this.place,
    fahrenheit: fahrenheit ?? this.fahrenheit,
    counter: counter ?? this.counter,
    counterLabel: counterLabel ?? this.counterLabel,
    statusText: statusText ?? this.statusText,
    statusDnd: statusDnd ?? this.statusDnd,
  );

  /// The widgets the scheduler keeps up to date: switched on and with a slot.
  List<String> get rotating => [
    for (final id in WidgetIds.rotating)
      if (rotation.contains(id) && slots.containsKey(id)) id,
  ];

  Map<String, Object?> toJson() => {
    'autoOn': autoOn,
    'refreshMins': refreshMins,
    'rotateSecs': rotateSecs,
    'rotateHome': rotateHome,
    'musicOn': musicOn,
    'slots': slots,
    'rotation': rotation.toList()..sort(),
    'place': place?.toJson(),
    'fahrenheit': fahrenheit,
    'counter': counter,
    'counterLabel': counterLabel,
    'statusText': statusText,
    'statusDnd': statusDnd,
  };

  static WidgetSettings fromJson(Object? j) {
    if (j is! Map) return const WidgetSettings();
    int intOr(Object? v, int d, [List<int>? allowed]) {
      final n = v is num ? v.toInt() : d;
      return allowed == null || allowed.contains(n) ? n : d;
    }

    final slots = <String, int>{};
    final rawSlots = j['slots'];
    if (rawSlots is Map) {
      rawSlots.forEach((k, v) {
        if (k is String && v is num && v >= 0 && v < 16) slots[k] = v.toInt();
      });
    }
    final rotation = <String>{
      if (j['rotation'] is List)
        for (final r in j['rotation'] as List)
          if (r is String) r,
    };
    return WidgetSettings(
      autoOn: j['autoOn'] == true,
      refreshMins: intOr(j['refreshMins'], 30, refreshChoices),
      rotateSecs: intOr(j['rotateSecs'], 60, rotateChoices),
      rotateHome: j['rotateHome'] != false,
      musicOn: j['musicOn'] == true,
      slots: slots,
      rotation: rotation,
      place: GeoPlace.fromJson(j['place']),
      fahrenheit: j['fahrenheit'] == true,
      counter: intOr(j['counter'], 0).clamp(-9999, 99999),
      counterLabel: (j['counterLabel'] as String?) ?? 'STREAK',
      statusText: (j['statusText'] as String?) ?? 'IN A MEETING',
      statusDnd: j['statusDnd'] == true,
    );
  }
}

/// What the background scheduler last did, for the app to show.
class BgStatus {
  final DateTime? at;
  final DateTime? lastRefresh;
  final int? showingSlot;
  final int? homeSlot;
  final String? error;
  const BgStatus({this.at, this.lastRefresh, this.showingSlot, this.homeSlot, this.error});

  Map<String, Object?> toJson() => {
    'at': at?.millisecondsSinceEpoch,
    'lastRefresh': lastRefresh?.millisecondsSinceEpoch,
    'showingSlot': showingSlot,
    'homeSlot': homeSlot,
    'error': error,
  };

  static BgStatus fromJson(Object? j) {
    if (j is! Map) return const BgStatus();
    DateTime? t(Object? v) => v is num ? DateTime.fromMillisecondsSinceEpoch(v.toInt()) : null;
    return BgStatus(
      at: t(j['at']),
      lastRefresh: t(j['lastRefresh']),
      showingSlot: (j['showingSlot'] as num?)?.toInt(),
      homeSlot: (j['homeSlot'] as num?)?.toInt(),
      error: j['error'] as String?,
    );
  }
}

abstract class SettingsStore {
  Future<WidgetSettings> load();
  Future<void> save(WidgetSettings s);
  Future<BgStatus> loadStatus();
  Future<void> saveStatus(BgStatus s);
}

class PrefsSettingsStore implements SettingsStore {
  static const _kSettings = 'widgets_settings_v1', _kStatus = 'widgets_status_v1';
  final SharedPreferencesAsync _p = SharedPreferencesAsync();

  Future<Object?> _read(String key) async {
    try {
      final raw = await _p.getString(key);
      return raw == null ? null : jsonDecode(raw);
    } on Object {
      return null;
    }
  }

  @override
  Future<WidgetSettings> load() async => WidgetSettings.fromJson(await _read(_kSettings));

  @override
  Future<void> save(WidgetSettings s) async {
    try {
      await _p.setString(_kSettings, jsonEncode(s.toJson()));
    } on Object {
      // Not stored (no storage here): the in-memory copy still works this run.
    }
  }

  @override
  Future<BgStatus> loadStatus() async => BgStatus.fromJson(await _read(_kStatus));

  @override
  Future<void> saveStatus(BgStatus s) async {
    try {
      await _p.setString(_kStatus, jsonEncode(s.toJson()));
    } on Object {
      // Status is informational only.
    }
  }
}

class MemorySettingsStore implements SettingsStore {
  WidgetSettings settings;
  BgStatus status = const BgStatus();
  int saves = 0;
  MemorySettingsStore([this.settings = const WidgetSettings()]);

  @override
  Future<WidgetSettings> load() async => settings;

  @override
  Future<void> save(WidgetSettings s) async {
    settings = s;
    saves++;
  }

  @override
  Future<BgStatus> loadStatus() async => status;

  @override
  Future<void> saveStatus(BgStatus s) async => status = s;
}
