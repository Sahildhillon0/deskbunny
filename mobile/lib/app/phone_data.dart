// What the phone knows that the widgets can show: its battery, the next
// calendar event and the song that is playing. Read through the native
// `nixxa/phone` channel (Kotlin: PhoneChannel.kt); nothing here touches the
// network.
import 'dart:async';

import 'package:flutter/services.dart';

class PhoneException implements Exception {
  /// NO_ACCESS (notification access for the song), NO_PERMISSION (calendar), or UNAVAILABLE.
  final String code;
  final String message;
  const PhoneException(this.code, this.message);
  @override
  String toString() => message;
}

class PhoneBattery {
  final int percent; // 0..100
  final bool charging;
  const PhoneBattery(this.percent, this.charging);
}

class CalendarEvent {
  final String title;
  final DateTime start;
  final bool allDay;
  const CalendarEvent(this.title, this.start, {this.allDay = false});
}

class NowPlaying {
  final String title, artist;

  /// Label of the app playing it ("Spotify"), "" when unknown.
  final String app;
  final int durationMs; // 0 when unknown (a live stream)
  final int positionMs;
  final bool playing;
  const NowPlaying({
    required this.title,
    required this.artist,
    this.app = '',
    this.durationMs = 0,
    this.positionMs = 0,
    required this.playing,
  });

  /// Same song, same app: what "the song changed" means.
  String get key => '$app|$title|$artist';

  @override
  bool operator ==(Object other) =>
      other is NowPlaying &&
      other.title == title &&
      other.artist == artist &&
      other.app == app &&
      other.durationMs == durationMs &&
      other.positionMs == positionMs &&
      other.playing == playing;

  @override
  int get hashCode => Object.hash(title, artist, app, durationMs, positionMs, playing);
}

abstract class PhoneApi {
  Future<PhoneBattery> battery();

  Future<bool> calendarAllowed();

  /// Asks Android for calendar access. Only works from the app's own screen.
  Future<bool> requestCalendar();

  /// The next event starting within a week, or null.
  Future<CalendarEvent?> nextEvent();

  /// The song playing now (or paused), or null when no player has one.
  /// Throws [PhoneException] NO_ACCESS without notification access.
  Future<NowPlaying?> nowPlaying();

  /// Fires when the song or its play state changes.
  Stream<void> get nowPlayingChanges;
}

class ChannelPhoneApi implements PhoneApi {
  ChannelPhoneApi({
    MethodChannel methodChannel = const MethodChannel('nixxa/phone'),
    EventChannel eventChannel = const EventChannel('nixxa/phone/nowplaying'),
  }) : _m = methodChannel,
       _events = eventChannel;

  final MethodChannel _m;
  final EventChannel _events;

  Future<T?> _call<T>(String method, [Object? args]) async {
    try {
      return await _m.invokeMethod<T>(method, args);
    } on PlatformException catch (e) {
      throw PhoneException(e.code, e.message ?? e.code);
    } on MissingPluginException {
      throw const PhoneException('UNAVAILABLE', 'This needs the Android app.');
    }
  }

  @override
  Future<PhoneBattery> battery() async {
    final m = await _call<Map<Object?, Object?>>('battery');
    if (m == null) throw const PhoneException('UNAVAILABLE', 'The battery level is not available.');
    return PhoneBattery((m['percent'] as num).toInt().clamp(0, 100), m['charging'] == true);
  }

  @override
  Future<bool> calendarAllowed() async => await _call<bool>('calendarAllowed') ?? false;

  @override
  Future<bool> requestCalendar() async => await _call<bool>('requestCalendar') ?? false;

  @override
  Future<CalendarEvent?> nextEvent() async {
    final m = await _call<Map<Object?, Object?>>('nextEvent');
    if (m == null) return null;
    return CalendarEvent(
      (m['title'] as String?) ?? '',
      DateTime.fromMillisecondsSinceEpoch((m['startMs'] as num).toInt()),
      allDay: m['allDay'] == true,
    );
  }

  @override
  Future<NowPlaying?> nowPlaying() async {
    final m = await _call<Map<Object?, Object?>>('nowPlaying');
    if (m == null) return null;
    return NowPlaying(
      title: (m['title'] as String?) ?? '',
      artist: (m['artist'] as String?) ?? '',
      app: (m['app'] as String?) ?? '',
      durationMs: (m['durationMs'] as num?)?.toInt() ?? 0,
      positionMs: (m['positionMs'] as num?)?.toInt() ?? 0,
      playing: m['playing'] == true,
    );
  }

  @override
  Stream<void> get nowPlayingChanges => _events.receiveBroadcastStream().map((_) {});
}
