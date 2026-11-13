// Widgets: screens the bunny can show instead of a face. The clock is drawn by
// the bunny itself. The rest are pictures the phone makes (from the internet,
// from the phone, or typed by you) and puts in a slot. Auto-update keeps the
// ones you chose fresh and rotates the bunny through them, in the background.
import 'dart:async';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../app/theme.dart';
import '../app/widget_settings.dart';
import 'clock_screen.dart';
import 'data_widget_screen.dart';
import 'phone_widget_screens.dart';
import 'widgets/common.dart';

class WidgetsScreen extends StatefulWidget {
  const WidgetsScreen({super.key});

  @override
  State<WidgetsScreen> createState() => _WidgetsScreenState();
}

class _WidgetsScreenState extends State<WidgetsScreen> {
  Timer? _poll;

  @override
  void initState() {
    super.initState();
    final app = context.read<AppState>();
    unawaited(app.refreshBgStatus());
    // The background scheduler reports back through storage: look now and then.
    _poll = Timer.periodic(const Duration(seconds: 5), (_) => app.refreshBgStatus());
  }

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }

  String _time(DateTime t) =>
      '${t.hour.toString().padLeft(2, '0')}:${t.minute.toString().padLeft(2, '0')}';

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    final s = app.settings;
    void open(Widget screen) => Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => screen));
    final city = app.place?.name;
    final st = app.bgStatus;
    final chosen = s.rotating.length;
    return Scaffold(
      appBar: AppBar(title: const Text('Widgets')),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
        children: [
          const SectionLabel('Auto-update'),
          SwitchRow(
            key: const Key('auto-on'),
            label: 'Keep widgets updated and rotate',
            hint: 'The bunny cycles through the widgets you switch on, and nixxa refreshes them in the background.',
            value: s.autoOn,
            onChanged: (v) => app.updateSettings((s) => s.copyWith(autoOn: v)),
          ),
          if (s.autoOn) ...[
            const SizedBox(height: 4),
            _Choice<int>(
              key: const Key('auto-refresh'),
              label: 'Refresh every',
              value: s.refreshMins,
              options: {for (final m in refreshChoices) m: m < 60 ? '$m min' : '${m ~/ 60} h'},
              onChanged: (v) => app.updateSettings((s) => s.copyWith(refreshMins: v)),
            ),
            const SizedBox(height: 8),
            _Choice<int>(
              key: const Key('auto-rotate'),
              label: 'Switch every',
              value: s.rotateSecs,
              options: {for (final x in rotateChoices) x: x < 60 ? '$x s' : '${x ~/ 60} min'},
              onChanged: (v) => app.updateSettings((s) => s.copyWith(rotateSecs: v)),
            ),
            SwitchRow(
              key: const Key('auto-home'),
              label: 'Show my face too',
              hint: 'Your current face is part of the cycle',
              value: s.rotateHome,
              onChanged: (v) => app.updateSettings((s) => s.copyWith(rotateHome: v)),
            ),
            Padding(
              padding: const EdgeInsets.only(top: 4),
              child: Text(
                chosen == 0
                    ? 'Nothing is switched on yet. Open a widget, send it once, then turn on "Keep it updated and rotate".'
                    : [
                        '$chosen ${chosen == 1 ? 'widget' : 'widgets'} in the rotation',
                        if (st.lastRefresh != null) 'last refreshed ${_time(st.lastRefresh!)}',
                        if (st.showingSlot != null) 'showing slot ${st.showingSlot! + 1}',
                      ].join(' · '),
                key: const Key('auto-status'),
                style: const TextStyle(fontSize: 12, color: NixxaColors.text2),
              ),
            ),
            if (st.error != null)
              Padding(
                padding: const EdgeInsets.only(top: 4),
                child: Text(st.error!, key: const Key('auto-error'), style: const TextStyle(fontSize: 12, color: NixxaColors.warn)),
              ),
            const Padding(
              padding: EdgeInsets.only(top: 6),
              child: Text(
                'It runs while nixxa keeps your bunny connected, even with the app closed. '
                'Switching face is saved on the bunny each time, so the fastest cycle is every 30 seconds.',
                style: TextStyle(fontSize: 12, color: NixxaColors.text3, height: 1.4),
              ),
            ),
          ],
          const SectionLabel('On the bunny'),
          _Row(
            key: const Key('widgets-clock'),
            icon: Icons.schedule,
            title: 'Clock',
            subtitle: app.clockStyle == 0 ? 'Shows the face' : app.clockRenderer.name(app.clockStyle),
            onTap: () => open(const ClockScreen()),
          ),
          _Row(
            key: const Key('widgets-nowplaying'),
            icon: Icons.music_note_outlined,
            title: 'Now playing',
            subtitle: s.musicOn ? 'Following the music' : 'The song from Spotify, YT Music…',
            onTap: () => open(const NowPlayingScreen()),
          ),
          const SectionLabel('From the internet'),
          _Row(
            key: const Key('widgets-weather'),
            icon: Icons.wb_sunny_outlined,
            title: 'Weather',
            subtitle: city ?? 'Pick a city',
            onTap: () => open(const DataWidgetScreen(kind: DataWidgetKind.weather)),
          ),
          _Row(
            key: const Key('widgets-air'),
            icon: Icons.air,
            title: 'Air quality',
            subtitle: city ?? 'Pick a city',
            onTap: () => open(const DataWidgetScreen(kind: DataWidgetKind.air)),
          ),
          const SectionLabel('From your phone'),
          _Row(
            key: const Key('widgets-battery'),
            icon: Icons.battery_5_bar_outlined,
            title: 'Phone battery',
            subtitle: 'Charge and charging',
            onTap: () => open(const BatteryScreen()),
          ),
          _Row(
            key: const Key('widgets-event'),
            icon: Icons.event_outlined,
            title: 'Next event',
            subtitle: 'From your calendar',
            onTap: () => open(const EventScreen()),
          ),
          const SectionLabel('Yours'),
          _Row(
            key: const Key('widgets-counter'),
            icon: Icons.exposure_plus_1,
            title: 'Counter',
            subtitle: '${s.counterLabel.isEmpty ? 'COUNT' : s.counterLabel}: ${s.counter}',
            onTap: () => open(const CounterScreen()),
          ),
          _Row(
            key: const Key('widgets-status'),
            icon: Icons.do_not_disturb_on_outlined,
            title: 'Status sign',
            subtitle: s.statusText,
            onTap: () => open(const StatusScreen()),
          ),
          const SizedBox(height: 12),
          const Text(
            'Weather and air quality use the internet. Next event asks to read your calendar. '
            'Now playing uses notification access. Each widget is a picture that nixxa makes and puts in a slot.',
            style: TextStyle(fontSize: 12, color: NixxaColors.text3, height: 1.4),
          ),
        ],
      ),
    );
  }
}

class _Row extends StatelessWidget {
  final IconData icon;
  final String title, subtitle;
  final VoidCallback onTap;
  const _Row({super.key, required this.icon, required this.title, required this.subtitle, required this.onTap});

  @override
  Widget build(BuildContext context) => Card(
    child: ListTile(
      leading: Icon(icon),
      title: Text(title),
      subtitle: Text(subtitle, style: const TextStyle(fontSize: 12, color: NixxaColors.text3)),
      trailing: const Icon(Icons.chevron_right, color: NixxaColors.text3),
      onTap: onTap,
    ),
  );
}

/// A label with a few mutually exclusive choices.
class _Choice<T> extends StatelessWidget {
  final String label;
  final T value;
  final Map<T, String> options;
  final ValueChanged<T> onChanged;
  const _Choice({super.key, required this.label, required this.value, required this.options, required this.onChanged});

  @override
  Widget build(BuildContext context) => Row(
    children: [
      SizedBox(width: 108, child: Text(label, style: const TextStyle(color: NixxaColors.text2, fontSize: 13))),
      Expanded(
        child: SegmentedButton<T>(
          segments: [for (final e in options.entries) ButtonSegment(value: e.key, label: Text(e.value))],
          selected: {value},
          showSelectedIcon: false,
          style: const ButtonStyle(visualDensity: VisualDensity.compact),
          onSelectionChanged: (s) => onChanged(s.first),
        ),
      ),
    ],
  );
}
