// The widgets that come from the phone itself: battery, next calendar event,
// a counter you set, a status sign, and the song that is playing.
import 'dart:async';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../app/labels.dart';
import '../app/phone_data.dart';
import '../app/theme.dart';
import '../app/widget_render.dart';
import '../app/widget_settings.dart';
import 'widget_parts.dart';
import 'widgets/common.dart';

// ------------------------------------------------------------------ battery

class BatteryScreen extends StatefulWidget {
  const BatteryScreen({super.key});

  @override
  State<BatteryScreen> createState() => _BatteryScreenState();
}

class _BatteryScreenState extends State<BatteryScreen> {
  PhoneBattery? _battery;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final b = await context.read<AppState>().phone.battery();
      if (mounted) {
        setState(() {
          _battery = b;
          _error = null;
        });
      }
    } on Object catch (e) {
      if (mounted) setState(() => _error = errorMessage(e));
    }
  }

  @override
  Widget build(BuildContext context) {
    final b = _battery;
    return Scaffold(
      appBar: AppBar(title: const Text('Phone battery')),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
        children: [
          const SectionLabel('On the bunny'),
          WidgetPreview(frame: b == null ? null : renderBattery(b), empty: 'Reading the battery…'),
          if (_error != null) _Problem(_error!),
          WidgetSendPanel(id: WidgetIds.battery, frame: b == null ? null : renderBattery(b), onRefresh: _load),
          const _Note('Shows your phone\'s charge and whether it is charging. Auto-update redraws it every few minutes.'),
        ],
      ),
    );
  }
}

// ------------------------------------------------------------------ next event

class EventScreen extends StatefulWidget {
  const EventScreen({super.key});

  @override
  State<EventScreen> createState() => _EventScreenState();
}

class _EventScreenState extends State<EventScreen> {
  bool? _allowed;
  CalendarEvent? _event;
  bool _loaded = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final phone = context.read<AppState>().phone;
    try {
      final allowed = await phone.calendarAllowed();
      final event = allowed ? await phone.nextEvent() : null;
      if (mounted) {
        setState(() {
          _allowed = allowed;
          _event = event;
          _loaded = true;
          _error = null;
        });
      }
    } on Object catch (e) {
      if (mounted) setState(() => _error = errorMessage(e));
    }
  }

  Future<void> _allow() async {
    await guard(context, () async {
      await context.read<AppState>().phone.requestCalendar();
    });
    await _load();
  }

  @override
  Widget build(BuildContext context) {
    final frame = _allowed == true && _loaded ? renderEvent(_event, DateTime.now()) : null;
    return Scaffold(
      appBar: AppBar(title: const Text('Next event')),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
        children: [
          if (_allowed == false) ...[
            const SectionLabel('Calendar access'),
            _Card(
              title: 'nixxa can\'t read your calendar yet',
              body: 'Android asks you to allow it. nixxa only looks at what starts in the next seven days, on this phone, '
                  'and never sends it anywhere except as a picture to your bunny.',
              button: FilledButton(key: const Key('ev-allow'), onPressed: _allow, child: const Text('Allow calendar access')),
            ),
          ],
          const SectionLabel('On the bunny'),
          WidgetPreview(frame: frame, empty: _allowed == false ? 'Calendar access is off' : 'Reading your calendar…'),
          if (_error != null) _Problem(_error!),
          WidgetSendPanel(id: WidgetIds.event, frame: frame, onRefresh: _load),
          const _Note('Shows the next event that starts within a week.'),
        ],
      ),
    );
  }
}

// ------------------------------------------------------------------ counter

class CounterScreen extends StatefulWidget {
  const CounterScreen({super.key});

  @override
  State<CounterScreen> createState() => _CounterScreenState();
}

class _CounterScreenState extends State<CounterScreen> {
  late final TextEditingController _label = TextEditingController(text: context.read<AppState>().settings.counterLabel);

  @override
  void dispose() {
    _label.dispose();
    super.dispose();
  }

  void _bump(AppState app, int by) =>
      app.updateSettings((s) => s.copyWith(counter: (s.counter + by).clamp(-9999, 99999)));

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    final s = app.settings;
    final frame = renderCounter(s.counter, s.counterLabel);
    return Scaffold(
      appBar: AppBar(title: const Text('Counter')),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
        children: [
          const SectionLabel('What are you counting?'),
          TextField(
            key: const Key('ct-label'),
            controller: _label,
            maxLength: 20,
            textCapitalization: TextCapitalization.characters,
            decoration: const InputDecoration(hintText: 'Day streak, glasses of water…'),
            onChanged: (v) => app.updateSettings((s) => s.copyWith(counterLabel: v.toUpperCase())),
          ),
          const SectionLabel('Count'),
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              IconButton.filledTonal(
                key: const Key('ct-minus'),
                tooltip: 'One less',
                onPressed: () => _bump(app, -1),
                icon: const Icon(Icons.remove),
              ),
              Text('${s.counter}', key: const Key('ct-value'), style: const TextStyle(fontSize: 40, fontWeight: FontWeight.w600)),
              IconButton.filledTonal(
                key: const Key('ct-plus'),
                tooltip: 'One more',
                onPressed: () => _bump(app, 1),
                icon: const Icon(Icons.add),
              ),
            ],
          ),
          Align(
            alignment: Alignment.centerRight,
            child: TextButton(
              key: const Key('ct-reset'),
              onPressed: s.counter == 0 ? null : () => app.updateSettings((s) => s.copyWith(counter: 0)),
              child: const Text('Reset to 0'),
            ),
          ),
          const SectionLabel('On the bunny'),
          WidgetPreview(frame: frame, empty: ''),
          WidgetSendPanel(id: WidgetIds.counter, frame: frame),
          const _Note('With auto-update on, the bunny shows the new number a few seconds after you change it.'),
        ],
      ),
    );
  }
}

// ------------------------------------------------------------------ status sign

class StatusScreen extends StatefulWidget {
  const StatusScreen({super.key});

  @override
  State<StatusScreen> createState() => _StatusScreenState();
}

class _StatusScreenState extends State<StatusScreen> {
  late final TextEditingController _text = TextEditingController(text: context.read<AppState>().settings.statusText);

  @override
  void dispose() {
    _text.dispose();
    super.dispose();
  }

  void _set(AppState app, String v) => app.updateSettings((s) => s.copyWith(statusText: v.toUpperCase()));

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    final s = app.settings;
    final frame = renderStatus(s.statusText);
    return Scaffold(
      appBar: AppBar(title: const Text('Status sign')),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
        children: [
          const SectionLabel('Pick a sign'),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              for (var i = 0; i < statusPresets.length; i++)
                ChoiceChip(
                  key: Key('st-preset-$i'),
                  label: Text(statusPresets[i]),
                  selected: s.statusText == statusPresets[i],
                  onSelected: (_) {
                    _text.text = statusPresets[i];
                    _set(app, statusPresets[i]);
                  },
                ),
            ],
          ),
          const SectionLabel('Or write your own'),
          TextField(
            key: const Key('st-text'),
            controller: _text,
            maxLength: 24,
            textCapitalization: TextCapitalization.characters,
            decoration: const InputDecoration(hintText: 'GONE FISHING'),
            onChanged: (v) => _set(app, v),
          ),
          SwitchRow(
            key: const Key('st-dnd'),
            label: 'Also turn on Do Not Disturb',
            hint: 'The bunny stops showing notification cards when you send the sign',
            value: s.statusDnd,
            onChanged: (v) => app.updateSettings((s) => s.copyWith(statusDnd: v)),
          ),
          const SectionLabel('On the bunny'),
          WidgetPreview(frame: frame, empty: ''),
          WidgetSendPanel(
            id: WidgetIds.status,
            frame: frame,
            canRotate: false,
            afterSend: () async {
              if (app.settings.statusDnd) await guard(context, () => app.setDnd(true), success: 'Do Not Disturb is on');
            },
          ),
          const _Note('A sign stays up until you send something else or switch the bunny back to a face.'),
        ],
      ),
    );
  }
}

// ------------------------------------------------------------------ now playing

class NowPlayingScreen extends StatefulWidget {
  const NowPlayingScreen({super.key});

  @override
  State<NowPlayingScreen> createState() => _NowPlayingScreenState();
}

class _NowPlayingScreenState extends State<NowPlayingScreen> with WidgetsBindingObserver {
  NowPlaying? _song;
  bool? _access;
  String? _error;
  Timer? _timer;
  StreamSubscription<void>? _sub;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    final app = context.read<AppState>();
    _timer = Timer.periodic(const Duration(seconds: 3), (_) => _poll());
    _sub = app.phone.nowPlayingChanges.listen((_) => _poll(), onError: (Object _) {});
    _check();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _timer?.cancel();
    _sub?.cancel();
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    // Back from the notification-access settings page.
    if (state == AppLifecycleState.resumed) _check();
  }

  Future<void> _check() async {
    try {
      final a = await context.read<AppState>().link.hasNotificationAccess();
      if (mounted) setState(() => _access = a);
      if (a) await _poll();
    } on Object {
      // keep the last answer
    }
  }

  Future<void> _poll() async {
    if (_access == false || !mounted) return;
    try {
      final song = await context.read<AppState>().phone.nowPlaying();
      if (mounted) {
        setState(() {
          _song = song;
          _error = null;
        });
      }
    } on PhoneException catch (e) {
      if (!mounted) return;
      setState(() {
        if (e.code == 'NO_ACCESS') _access = false;
        _error = e.code == 'NO_ACCESS' ? null : e.message;
      });
    } on Object catch (e) {
      if (mounted) setState(() => _error = errorMessage(e));
    }
  }

  Future<void> _follow(AppState app, bool on) async {
    if (!on) {
      app.updateSettings((s) => s.copyWith(musicOn: false));
      return;
    }
    final slot = app.settings.slots[WidgetIds.nowPlaying] ?? await chooseWidgetSlot(context, WidgetIds.nowPlaying);
    if (slot == null) return;
    app.assignSlot(WidgetIds.nowPlaying, slot);
    app.updateSettings((s) => s.copyWith(musicOn: true));
  }

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    final s = app.settings;
    final song = _song;
    final frame = song == null ? null : renderNowPlaying(song);
    final slot = s.slots[WidgetIds.nowPlaying];
    return Scaffold(
      appBar: AppBar(title: const Text('Now playing')),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
        children: [
          if (_access == false) ...[
            const SectionLabel('Notification access'),
            _Card(
              title: 'nixxa can\'t see what is playing yet',
              body: 'Android shares the current song (Spotify, YT Music and other players) with apps that have '
                  'notification access. nixxa already needs it for notifications on the bunny.',
              button: FilledButton(
                key: const Key('np-access'),
                onPressed: () async {
                  await guard(context, app.link.openNotificationAccessSettings);
                },
                child: const Text('Allow notification access'),
              ),
            ),
          ],
          const SectionLabel('Follow the music'),
          SwitchRow(
            key: const Key('np-follow'),
            label: 'Show the song on the bunny',
            hint: s.musicOn && slot != null
                ? 'Slot ${slot + 1}. Appears when a song starts, moves its bar every 10 seconds, '
                      'and your face comes back when you stop.'
                : 'It takes over one slot. The song appears when it starts and your face comes back when you stop.',
            value: s.musicOn,
            onChanged: _access == false ? null : (v) => _follow(app, v),
          ),
          const SectionLabel('Playing now'),
          WidgetPreview(frame: frame, empty: _access == false ? 'Notification access is off' : 'Nothing is playing'),
          if (_error != null) _Problem(_error!),
          WidgetSendPanel(id: WidgetIds.nowPlaying, frame: frame, canRotate: false),
          const _Note(
            'Works with any player that shows a media notification. Titles in scripts the bunny\'s small font can\'t draw '
            '(Devanagari, Hangul, Chinese…) fall back to the artist name, or show UNKNOWN TRACK.',
          ),
        ],
      ),
    );
  }
}

// ------------------------------------------------------------------ small pieces

class _Problem extends StatelessWidget {
  final String text;
  const _Problem(this.text);

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(top: 8),
    child: Text(text, style: const TextStyle(color: NixxaColors.danger, fontSize: 13)),
  );
}

class _Note extends StatelessWidget {
  final String text;
  const _Note(this.text);

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(top: 12),
    child: Text(text, style: const TextStyle(fontSize: 12, color: NixxaColors.text3, height: 1.4)),
  );
}

class _Card extends StatelessWidget {
  final String title, body;
  final Widget button;
  const _Card({required this.title, required this.body, required this.button});

  @override
  Widget build(BuildContext context) => Container(
    padding: const EdgeInsets.all(12),
    decoration: BoxDecoration(
      color: NixxaColors.warn.withValues(alpha: 0.08),
      borderRadius: BorderRadius.circular(8),
      border: Border.all(color: NixxaColors.warn.withValues(alpha: 0.35)),
    ),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(title, style: const TextStyle(color: NixxaColors.warn, fontWeight: FontWeight.w600)),
        const SizedBox(height: 4),
        Text(body, style: const TextStyle(fontSize: 13, color: NixxaColors.text2)),
        const SizedBox(height: 8),
        button,
      ],
    ),
  );
}
