// Test doubles shared by the app and UI tests: widget data without a network
// and phone data without a phone.
import 'dart:async';

import 'package:nixxa/app/phone_data.dart';
import 'package:nixxa/app/widget_data.dart';

/// Widget data without a network: one city, fixed readings, optional failure.
class FakeWidgetData implements WidgetData {
  int searches = 0, weatherCalls = 0, airCalls = 0;
  String? failWith;
  WeatherNow nowWeather = const WeatherNow(tempC: 21.4, code: 61, isDay: true, humidity: 70, windKmh: 9);
  AirNow nowAir = const AirNow(usAqi: 78, pm25: 22, pm10: 40);

  @override
  Future<List<GeoPlace>> search(String query) async {
    searches++;
    if (failWith != null) throw WidgetDataException(failWith!);
    if (query.trim().toLowerCase() == 'nowhere') return const [];
    return const [
      GeoPlace('Pune', 18.52, 73.86, region: 'Maharashtra', country: 'India'),
      GeoPlace('Punta Arenas', -53.16, -70.91, region: 'Magallanes', country: 'Chile'),
    ];
  }

  @override
  Future<WeatherNow> weather(GeoPlace place) async {
    weatherCalls++;
    if (failWith != null) throw WidgetDataException(failWith!);
    return nowWeather;
  }

  @override
  Future<AirNow> air(GeoPlace place) async {
    airCalls++;
    if (failWith != null) throw WidgetDataException(failWith!);
    return nowAir;
  }
}

/// The phone: a battery, a calendar and a music player you can steer.
class FakePhoneApi implements PhoneApi {
  PhoneBattery batteryNow = const PhoneBattery(64, false);
  bool calendar = true;
  CalendarEvent? event;
  NowPlaying? song;
  String? songFails; // PhoneException code
  int batteryCalls = 0, songCalls = 0;
  final _changes = StreamController<void>.broadcast();

  @override
  Future<PhoneBattery> battery() async {
    batteryCalls++;
    return batteryNow;
  }

  @override
  Future<bool> calendarAllowed() async => calendar;

  @override
  Future<bool> requestCalendar() async => calendar = true;

  @override
  Future<CalendarEvent?> nextEvent() async {
    if (!calendar) throw const PhoneException('NO_PERMISSION', 'Calendar access is off.');
    return event;
  }

  @override
  Future<NowPlaying?> nowPlaying() async {
    songCalls++;
    if (songFails != null) throw PhoneException(songFails!, 'Allow notification access to see the song.');
    return song;
  }

  /// Change the song and tell listeners, as the native side does.
  void play(NowPlaying? s) {
    song = s;
    _changes.add(null);
  }

  @override
  Stream<void> get nowPlayingChanges => _changes.stream;
}
