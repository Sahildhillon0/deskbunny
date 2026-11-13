// Weather and air quality: pick a city, see exactly what the bunny will show,
// and send it to a slot. The phone fetches the numbers and draws a picture, so
// the bunny shows a snapshot: send again to refresh it.
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../app/labels.dart';
import '../app/theme.dart';
import '../app/widget_data.dart';
import '../app/widget_render.dart';
import '../app/widget_settings.dart';
import 'widget_parts.dart';
import 'widgets/common.dart' show SectionLabel;

enum DataWidgetKind { weather, air }

class DataWidgetScreen extends StatefulWidget {
  final DataWidgetKind kind;
  const DataWidgetScreen({super.key, required this.kind});

  @override
  State<DataWidgetScreen> createState() => _DataWidgetScreenState();
}

class _DataWidgetScreenState extends State<DataWidgetScreen> {
  WeatherNow? _weather;
  AirNow? _air;
  bool _loading = false;
  String? _error;

  bool get _isWeather => widget.kind == DataWidgetKind.weather;
  String get _title => _isWeather ? 'Weather' : 'Air quality';

  @override
  void initState() {
    super.initState();
    if (context.read<AppState>().place != null) _refresh();
  }

  Uint8List? _frame(AppState app) {
    final place = app.place;
    if (place == null) return null;
    if (_isWeather) {
      final w = _weather;
      return w == null ? null : renderWeather(w, place.name, fahrenheit: app.fahrenheit);
    }
    final a = _air;
    return a == null ? null : renderAir(a, place.name);
  }

  Future<void> _refresh() async {
    final app = context.read<AppState>();
    final place = app.place;
    if (place == null || _loading) return;
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      if (_isWeather) {
        final w = await app.widgetData.weather(place);
        if (mounted && app.place == place) setState(() => _weather = w);
      } else {
        final a = await app.widgetData.air(place);
        if (mounted && app.place == place) setState(() => _air = a);
      }
    } on Object catch (e) {
      if (mounted) setState(() => _error = errorMessage(e));
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Future<void> _choose() async {
    final app = context.read<AppState>();
    final picked = await showModalBottomSheet<GeoPlace>(
      context: context,
      isScrollControlled: true,
      builder: (_) => const PlaceSheet(),
    );
    if (picked == null || !mounted) return;
    setState(() {
      _weather = null;
      _air = null;
    });
    app.setPlace(picked);
    await _refresh();
  }

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    final place = app.place;
    final frame = _frame(app);
    return Scaffold(
      appBar: AppBar(title: Text(_title)),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
        children: [
          const SectionLabel('City'),
          Card(
            child: ListTile(
              key: const Key('dw-place'),
              leading: const Icon(Icons.place_outlined),
              title: Text(place?.label ?? 'Choose a city'),
              trailing: const Icon(Icons.chevron_right, color: NixxaColors.text3),
              onTap: _choose,
            ),
          ),
          if (_isWeather) ...[
            const SizedBox(height: 8),
            SegmentedButton<bool>(
              key: const Key('dw-unit'),
              segments: const [
                ButtonSegment(value: false, label: Text('°C')),
                ButtonSegment(value: true, label: Text('°F')),
              ],
              selected: {app.fahrenheit},
              showSelectedIcon: false,
              onSelectionChanged: (s) => app.setFahrenheit(s.first),
            ),
          ],
          const SectionLabel('On the bunny'),
          WidgetPreview(
            frame: frame,
            loading: _loading,
            empty: place == null ? 'Choose a city first' : 'No reading yet',
          ),
          if (_error != null)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Text(_error!, key: const Key('dw-error'), style: const TextStyle(color: NixxaColors.danger, fontSize: 13)),
            ),
          WidgetSendPanel(
            id: _isWeather ? WidgetIds.weather : WidgetIds.air,
            frame: frame,
            onRefresh: place != null && !_loading ? _refresh : null,
          ),
          const SizedBox(height: 12),
          const Text(
            'The bunny shows a picture of the reading from when you sent it. Send again to update it. '
            'Numbers come from Open-Meteo; only the city you pick is sent.',
            style: TextStyle(fontSize: 12, color: NixxaColors.text3, height: 1.4),
          ),
        ],
      ),
    );
  }
}

/// Search for a city by name and pop the one picked.
class PlaceSheet extends StatefulWidget {
  const PlaceSheet({super.key});

  @override
  State<PlaceSheet> createState() => _PlaceSheetState();
}

class _PlaceSheetState extends State<PlaceSheet> {
  final _query = TextEditingController();
  List<GeoPlace>? _results;
  bool _busy = false;
  String? _error;

  @override
  void dispose() {
    _query.dispose();
    super.dispose();
  }

  Future<void> _search() async {
    final app = context.read<AppState>();
    FocusScope.of(context).unfocus();
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final r = await app.widgetData.search(_query.text);
      if (mounted) setState(() => _results = r);
    } on Object catch (e) {
      if (mounted) {
        setState(() {
          _results = null;
          _error = errorMessage(e);
        });
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final results = _results;
    return SafeArea(
      child: Padding(
        padding: EdgeInsets.fromLTRB(16, 0, 16, 16 + MediaQuery.viewInsetsOf(context).bottom),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            const Text('Choose a city', style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600)),
            const SizedBox(height: 8),
            TextField(
              key: const Key('place-query'),
              controller: _query,
              autofocus: true,
              textInputAction: TextInputAction.search,
              decoration: InputDecoration(
                hintText: 'City name',
                prefixIcon: const Icon(Icons.search, size: 20),
                suffixIcon: IconButton(
                  key: const Key('place-search'),
                  tooltip: 'Search',
                  onPressed: _busy ? null : _search,
                  icon: const Icon(Icons.arrow_forward),
                ),
              ),
              onSubmitted: (_) => _search(),
            ),
            const SizedBox(height: 8),
            if (_busy) const Padding(padding: EdgeInsets.all(16), child: Center(child: CircularProgressIndicator())),
            if (_error != null)
              Text(_error!, key: const Key('place-error'), style: const TextStyle(color: NixxaColors.danger, fontSize: 13)),
            if (results != null && results.isEmpty && !_busy)
              const Padding(
                padding: EdgeInsets.all(8),
                child: Text('No city found. Check the spelling.', style: TextStyle(color: NixxaColors.text3)),
              ),
            if (results != null && results.isNotEmpty)
              ConstrainedBox(
                constraints: const BoxConstraints(maxHeight: 320),
                child: ListView.builder(
                  shrinkWrap: true,
                  itemCount: results.length,
                  itemBuilder: (_, i) => ListTile(
                    key: Key('place-$i'),
                    leading: const Icon(Icons.place_outlined),
                    title: Text(results[i].name),
                    subtitle: Text(
                      [?results[i].region, ?results[i].country].join(', '),
                      style: const TextStyle(fontSize: 12, color: NixxaColors.text3),
                    ),
                    onTap: () => Navigator.pop(context, results[i]),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}
