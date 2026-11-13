// The widget scheduler: keeps the widgets you switched on fresh in their slots,
// rotates the bunny through them, and follows the song that is playing.
//
// It runs in the background Dart engine that the link service hosts
// (lib/background.dart), so it keeps going when the app is closed. It never
// holds settings itself: every pass reads them from the store, so a change in
// the app takes effect on the next pass. Everything it needs is injected, so
// the tests drive it with a fake clock, a fake bunny and fake phone data.
import 'dart:async';
import 'dart:typed_data';

import '../link/device_link.dart';
import '../model/drawing.dart';
import '../model/face.dart';
import '../protocol/bunny.dart';
import 'phone_data.dart';
import 'widget_data.dart';
import 'widget_render.dart';
import 'widget_settings.dart';

/// How long a paused song stays on the bunny before it lets go.
const Duration pausedGrace = Duration(seconds: 15);

/// How long rotation stays out of the way after you pick a face yourself.
const Duration userHold = Duration(minutes: 10);

/// Phone-side widgets (battery, counter, event) are cheap: refresh them often.
const Duration localRefresh = Duration(minutes: 5);

/// A failed update is tried again after this long.
const Duration retryAfter = Duration(minutes: 2);

/// While a song plays, its progress bar is redrawn this often. Starting,
/// changing, pausing and resuming are redrawn at once. Each redraw is a flash
/// write on the bunny, so this is not faster.
const Duration musicProgress = Duration(seconds: 10);

class WidgetScheduler {
  WidgetScheduler({
    required this.link,
    required this.bunny,
    required this.data,
    required this.phone,
    required this.store,
    this.tickEvery = const Duration(seconds: 5),
    DateTime Function()? clock,
  }) : _clock = clock ?? DateTime.now;

  final DeviceLink link;
  final Bunny bunny;
  final WidgetData data;
  final PhoneApi phone;
  final SettingsStore store;
  final Duration tickEvery;
  final DateTime Function() _clock;

  bool _connected = false;
  int? _showing; // the slot on screen, as far as we know
  int? _home; // the face to come back to
  bool _managed = false; // were we in charge on the last pass
  final Map<String, int> _pushed = {}; // widget id -> hash of the picture in its slot
  final Map<String, DateTime> _refreshedAt = {};
  final Map<String, String> _errors = {};
  DateTime? _lastRefresh, _lastRotate, _userHoldUntil, _pausedAt, _musicPushedAt;
  String? _musicKey;
  bool _musicShowing = false;
  bool _wasPlaying = false;
  String _lastStatus = '';

  bool _busy = false, _again = false;
  Timer? _timer;
  StreamSubscription<LinkStatus>? _linkSub;
  StreamSubscription<void>? _musicSub;

  int? get showingSlot => _showing;
  int? get homeSlot => _home;
  Map<String, String> get errors => Map.unmodifiable(_errors);

  void start() {
    _linkSub ??= link.status.listen((st) {
      final was = _connected;
      _connected = st.state == LinkState.connected;
      if (_connected && !was) {
        _showing = null; // look again: the bunny may have been powered off meanwhile
        unawaited(tick());
      }
    }, onError: (Object _) {});
    _musicSub ??= phone.nowPlayingChanges.listen((_) => unawaited(tick()), onError: (Object _) {});
    _timer ??= Timer.periodic(tickEvery, (_) => unawaited(tick()));
  }

  Future<void> stop() async {
    _timer?.cancel();
    _timer = null;
    await _linkSub?.cancel();
    _linkSub = null;
    await _musicSub?.cancel();
    _musicSub = null;
  }

  /// Marks the link connected without waiting for the status stream (tests).
  void setConnected(bool v) => _connected = v;

  /// One pass. Safe to call at any time and from anywhere: a call that lands
  /// while a pass is running asks for another pass right after it.
  Future<void> tick([DateTime? at]) async {
    if (_busy) {
      _again = true;
      return;
    }
    _busy = true;
    try {
      do {
        _again = false;
        await _pass(at ?? _clock());
      } while (_again);
    } on Object catch (e) {
      _errors['link'] = _say(e);
    } finally {
      _busy = false;
    }
  }

  // ---------------------------------------------------------------- the pass

  Future<void> _pass(DateTime now) async {
    final s = await store.load();
    final managed = s.autoOn || s.musicOn;
    // Off and was off: no Bluetooth traffic at all.
    if (!_connected || (!managed && !_managed)) return;
    _showing ??= (await bunny.getState()).activeSlot;
    _errors.remove('link');

    if (managed && !_managed) {
      // Taking over: remember the face to come back to.
      final widgetSlots = s.slots.values.toSet();
      _home = widgetSlots.contains(_showing) ? (await store.loadStatus()).homeSlot : _showing;
    }
    if (!managed && _managed) await _giveBack();
    _managed = managed;
    if (!managed) return _report(now);

    if (s.autoOn) await _refresh(s, now);
    final holding = await _music(s, now);
    if (s.autoOn && !holding) await _rotate(s, now);
    await _report(now);
  }

  /// Back to the face that was showing before we took over.
  Future<void> _giveBack() async {
    final home = _home;
    _musicShowing = false;
    _musicKey = null;
    _wasPlaying = false;
    if (home != null && _showing != home) {
      await _activate(home);
    }
  }

  Future<void> _activate(int slot) async {
    await bunny.activate(slot);
    _showing = slot;
  }

  // ---------------------------------------------------------------- refresh

  Future<void> _refresh(WidgetSettings s, DateTime now) async {
    for (final id in s.rotating) {
      // The counter is just what you typed: check it every pass (an unchanged
      // picture is never sent again).
      final every = id == WidgetIds.counter
          ? Duration.zero
          : WidgetIds.network.contains(id)
          ? Duration(minutes: s.refreshMins)
          : localRefresh;
      final last = _refreshedAt[id];
      if (last != null && now.difference(last) < every) continue;
      try {
        final frame = await _render(id, s, now);
        if (frame != null) await _push(id, s.slots[id]!, frame);
        _refreshedAt[id] = now;
        if (frame != null) _lastRefresh = now;
        _errors.remove(id);
      } on Object catch (e) {
        _errors[id] = '${WidgetIds.faceName(id)}: ${_say(e)}';
        _refreshedAt[id] = now.subtract(every).add(retryAfter);
      }
    }
  }

  Future<Uint8List?> _render(String id, WidgetSettings s, DateTime now) async {
    switch (id) {
      case WidgetIds.weather:
        final place = s.place;
        if (place == null) return null;
        return renderWeather(await data.weather(place), place.name, fahrenheit: s.fahrenheit);
      case WidgetIds.air:
        final place = s.place;
        if (place == null) return null;
        return renderAir(await data.air(place), place.name);
      case WidgetIds.battery:
        return renderBattery(await phone.battery());
      case WidgetIds.event:
        return renderEvent(await phone.nextEvent(), now);
      case WidgetIds.counter:
        return renderCounter(s.counter, s.counterLabel);
    }
    return null;
  }

  /// PUT_SLOT, unless that picture is already there. A slot that is on screen
  /// is activated again so the bunny shows the new picture.
  Future<void> _push(String id, int slot, Uint8List frame) async {
    final h = Object.hashAll(frame);
    if (_pushed[id] == h) return;
    await bunny.putSlot(
      slot,
      FaceDef(name: WidgetIds.faceName(id)),
      Drawing(mode: DrawingMode.replace, frameMs: 1000, frames: [frame]),
    );
    _pushed[id] = h;
  }

  // ---------------------------------------------------------------- music

  /// Follows the song. True while the song is on screen and should keep
  /// rotation from moving away from it.
  Future<bool> _music(WidgetSettings s, DateTime now) async {
    final slot = s.slots[WidgetIds.nowPlaying];
    if (!s.musicOn || slot == null) {
      _musicShowing = false;
      _musicKey = null;
      _wasPlaying = false;
      _errors.remove('music');
      return false;
    }
    final NowPlaying? np;
    try {
      np = await phone.nowPlaying();
      _errors.remove('music');
    } on Object catch (e) {
      _errors['music'] = 'Now playing: ${_say(e)}';
      return false;
    }
    if (np == null) {
      final wasShowing = _musicShowing;
      _musicShowing = false;
      _musicKey = null;
      _wasPlaying = false;
      if (wasShowing) await _release(s);
      return false;
    }

    final changed = np.key != _musicKey;
    final flipped = np.playing != _wasPlaying; // started, paused or resumed
    final started = np.playing && !_wasPlaying;
    _musicKey = np.key;
    _wasPlaying = np.playing;

    // Redraw at once when the song or its state changes, and every few
    // seconds while it is on screen to move the progress bar.
    final pushed = _musicPushedAt;
    final dueProgress = _showing == slot && (pushed == null || now.difference(pushed) >= musicProgress);
    if (changed || flipped || dueProgress) {
      try {
        await _push(WidgetIds.nowPlaying, slot, renderNowPlaying(np));
        _musicPushedAt = now;
      } on Object catch (e) {
        _errors['music'] = 'Now playing: ${_say(e)}';
        return false;
      }
    }

    if (np.playing && (changed || started)) {
      if (_showing != slot) await _activate(slot);
      _musicShowing = true;
      _pausedAt = null;
    }
    if (np.playing) {
      _pausedAt = null;
      return _musicShowing;
    }
    if (!_musicShowing) return false;
    _pausedAt ??= now;
    if (now.difference(_pausedAt!) >= pausedGrace) {
      _musicShowing = false;
      await _release(s);
      return false;
    }
    return true;
  }

  /// The song is over: rotation takes the screen back, or the face returns.
  Future<void> _release(WidgetSettings s) async {
    if (s.autoOn) {
      _lastRotate = null; // rotate on this very pass
    } else {
      await _giveBack();
    }
  }

  // ---------------------------------------------------------------- rotate

  Future<void> _rotate(WidgetSettings s, DateTime now) async {
    final entries = <int>[
      if (s.rotateHome && _home != null) _home!,
      for (final id in s.rotating)
        if (_pushed.containsKey(id)) s.slots[id]!,
    ];
    if (entries.length < 2) return;
    final hold = _userHoldUntil;
    if (hold != null && now.isBefore(hold)) return;

    final at = _showing == null ? -1 : entries.indexOf(_showing!);
    final last = _lastRotate;
    final due = last == null || at < 0 || now.difference(last) >= Duration(seconds: s.rotateSecs);
    if (!due) return;

    // If you picked some other face since we last looked, leave it alone.
    final actual = (await bunny.getState()).activeSlot;
    if (actual != _showing && !entries.contains(actual) && _showing != null) {
      _showing = actual;
      _userHoldUntil = now.add(userHold);
      return;
    }
    await _activate(entries[(at + 1) % entries.length]);
    _lastRotate = now;
  }

  // ---------------------------------------------------------------- status

  Future<void> _report(DateTime now) async {
    final st = BgStatus(
      at: now,
      lastRefresh: _lastRefresh,
      showingSlot: _showing,
      homeSlot: _home,
      error: _errors.values.isEmpty ? null : _errors.values.first,
    );
    // Not every pass: only when something worth showing changed.
    final key = '${st.lastRefresh?.millisecondsSinceEpoch}|${st.showingSlot}|${st.homeSlot}|${st.error}';
    if (key == _lastStatus) return;
    _lastStatus = key;
    await store.saveStatus(st);
  }

  static String _say(Object e) {
    if (e is WidgetDataException) return e.message;
    if (e is PhoneException) return e.message;
    if (e is LinkException) return e.code == 'NOT_CONNECTED' ? 'the bunny is not connected' : e.message;
    return 'could not update';
  }
}
