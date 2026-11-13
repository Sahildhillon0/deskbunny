// Data for the weather and air-quality widgets, from Open-Meteo (free, no API
// key). The only thing sent is the city name you search and the coordinates of
// the city you pick. The phone fetches; the bunny only ever gets a picture.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

/// A readable failure for the screens to show.
class WidgetDataException implements Exception {
  final String message;
  const WidgetDataException(this.message);
  @override
  String toString() => message;
}

class GeoPlace {
  final String name;
  final String? region;
  final String? country;
  final double lat, lon;
  const GeoPlace(this.name, this.lat, this.lon, {this.region, this.country});

  /// "Pune, Maharashtra, India".
  String get label => [name, if (region != null && region != name) region, ?country].join(', ');

  Map<String, Object?> toJson() => {'name': name, 'region': region, 'country': country, 'lat': lat, 'lon': lon};

  static GeoPlace? fromJson(Object? j) {
    if (j is! Map) return null;
    final name = j['name'], lat = j['lat'], lon = j['lon'];
    if (name is! String || lat is! num || lon is! num) return null;
    return GeoPlace(
      name,
      lat.toDouble(),
      lon.toDouble(),
      region: j['region'] as String?,
      country: j['country'] as String?,
    );
  }

  @override
  bool operator ==(Object other) => other is GeoPlace && other.name == name && other.lat == lat && other.lon == lon;

  @override
  int get hashCode => Object.hash(name, lat, lon);
}

class WeatherNow {
  final double tempC;
  final int code; // WMO weather code
  final bool isDay;
  final int humidity; // %
  final double windKmh;
  const WeatherNow({
    required this.tempC,
    required this.code,
    required this.isDay,
    required this.humidity,
    required this.windKmh,
  });
}

class AirNow {
  final int usAqi;
  final double? pm25, pm10;
  const AirNow({required this.usAqi, this.pm25, this.pm10});
}

/// Where the widgets get their numbers. Tests swap in a fake.
abstract class WidgetData {
  Future<List<GeoPlace>> search(String query);
  Future<WeatherNow> weather(GeoPlace place);
  Future<AirNow> air(GeoPlace place);
}

typedef JsonGet = Future<Object?> Function(Uri uri);

class OpenMeteo implements WidgetData {
  final JsonGet _get;
  OpenMeteo({JsonGet? get}) : _get = get ?? _httpGetJson;

  static Future<Object?> _httpGetJson(Uri uri) async {
    final client = HttpClient()..connectionTimeout = const Duration(seconds: 10);
    try {
      final req = await client.getUrl(uri).timeout(const Duration(seconds: 10));
      req.headers.set(HttpHeaders.userAgentHeader, 'nixxa');
      final res = await req.close().timeout(const Duration(seconds: 15));
      if (res.statusCode != 200) {
        await res.drain<void>();
        throw WidgetDataException('The weather service answered ${res.statusCode}. Try again later.');
      }
      final body = await res.transform(utf8.decoder).join().timeout(const Duration(seconds: 15));
      return jsonDecode(body);
    } on WidgetDataException {
      rethrow;
    } on TimeoutException {
      throw const WidgetDataException('The weather service did not answer in time. Check your connection.');
    } on SocketException {
      throw const WidgetDataException('No internet connection.');
    } on HandshakeException {
      throw const WidgetDataException('Could not make a secure connection to the weather service.');
    } on FormatException {
      throw const WidgetDataException('The weather service sent something unreadable.');
    } finally {
      client.close(force: true);
    }
  }

  Future<Map<String, Object?>> _object(Uri uri) async {
    final j = await _get(uri);
    if (j is! Map<String, Object?>) throw const WidgetDataException('The weather service sent something unreadable.');
    return j;
  }

  @override
  Future<List<GeoPlace>> search(String query) async {
    final q = query.trim();
    if (q.length < 2) throw const WidgetDataException('Type at least two letters of the city.');
    final j = await _object(
      Uri.https('geocoding-api.open-meteo.com', '/v1/search', {'name': q, 'count': '8', 'language': 'en', 'format': 'json'}),
    );
    final results = j['results'];
    if (results is! List) return const [];
    return [
      for (final r in results)
        if (r is Map && r['name'] is String && r['latitude'] is num && r['longitude'] is num)
          GeoPlace(
            r['name'] as String,
            (r['latitude'] as num).toDouble(),
            (r['longitude'] as num).toDouble(),
            region: r['admin1'] as String?,
            country: r['country'] as String?,
          ),
    ];
  }

  @override
  Future<WeatherNow> weather(GeoPlace place) async {
    final j = await _object(
      Uri.https('api.open-meteo.com', '/v1/forecast', {
        'latitude': place.lat.toStringAsFixed(4),
        'longitude': place.lon.toStringAsFixed(4),
        'current': 'temperature_2m,relative_humidity_2m,weather_code,is_day,wind_speed_10m',
        'wind_speed_unit': 'kmh',
      }),
    );
    final c = j['current'];
    if (c is! Map || c['temperature_2m'] is! num || c['weather_code'] is! num) {
      throw const WidgetDataException('No weather data for that place right now.');
    }
    return WeatherNow(
      tempC: (c['temperature_2m'] as num).toDouble(),
      code: (c['weather_code'] as num).toInt(),
      isDay: (c['is_day'] as num?)?.toInt() != 0,
      humidity: ((c['relative_humidity_2m'] as num?) ?? 0).round(),
      windKmh: ((c['wind_speed_10m'] as num?) ?? 0).toDouble(),
    );
  }

  @override
  Future<AirNow> air(GeoPlace place) async {
    final j = await _object(
      Uri.https('air-quality-api.open-meteo.com', '/v1/air-quality', {
        'latitude': place.lat.toStringAsFixed(4),
        'longitude': place.lon.toStringAsFixed(4),
        'current': 'us_aqi,pm2_5,pm10',
      }),
    );
    final c = j['current'];
    if (c is! Map || c['us_aqi'] is! num) {
      throw const WidgetDataException('No air-quality reading for that place. Try a bigger city nearby.');
    }
    return AirNow(
      usAqi: (c['us_aqi'] as num).round(),
      pm25: (c['pm2_5'] as num?)?.toDouble(),
      pm10: (c['pm10'] as num?)?.toDouble(),
    );
  }
}
