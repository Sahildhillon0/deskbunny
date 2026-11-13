// App-wide state: the link status, what's on the bunny, the local library and
// the notification config. Screens read it through provider and call its
// methods; errors propagate to the caller (see ui/widgets/common.dart guard()).
import 'dart:async';

import 'package:flutter/foundation.dart';

import '../link/device_link.dart';
import '../model/drawing.dart';
import '../model/face.dart';
import '../protocol/bunny.dart';
import 'face_store.dart';
import 'preview.dart';

const int opSetDnd = 0x23;

class AppState extends ChangeNotifier {
  final DeviceLink link;
  final Bunny bunny;
  final FaceStore store;
  final PreviewFactory previewFactory;
  final ClockRenderer clockRenderer;

  AppState({
    required this.link,
    Bunny? bunny,
    FaceStore? store,
    this.previewFactory = enginePreview,
    this.clockRenderer = engineClocks,
  }) : bunny = bunny ?? Bunny(link),
       store = store ?? PrefsFaceStore();

  LinkStatus status = const LinkStatus(LinkState.unpaired);

  /// False until the link reported its status once (splash until then).
  bool statusKnown = false;

  DeviceState? device;
  List<SlotInfo> slots = const [];
  SavedFace? activeFace;
  bool refreshing = false;
  String? refreshError;

  /// What each used slot holds, for the Library previews. Filled in the
  /// background after a refresh (one GET_SLOT per slot); a slot without an
  /// entry yet shows its name only.
  final Map<int, SavedFace> slotFaces = {};
  bool _loadingPreviews = false;
  bool _previewsDirty = false;

  List<SavedFace> saved = const [];
  NotifConfig? notifConfig;

  /// The clock the phone shows on the bunny: 0 = the face, 1..20 = a clock.
  int clockStyle = 0;

  StreamSubscription<LinkStatus>? _sub;
  bool _disposed = false;

  bool get paired => status.state != LinkState.unpaired;
  bool get connected => status.state == LinkState.connected;

  Future<void> start() async {
    _sub = link.status.listen(_onStatus, onError: (Object _) {});
    unawaited(reloadSaved());
    unawaited(loadClock());
    try {
      _onStatus(await link.currentStatus());
    } on Object {
      if (!statusKnown) {
        statusKnown = true;
        _notify();
      }
    }
  }

  void _onStatus(LinkStatus s) {
    final wasConnected = connected;
    status = s;
    statusKnown = true;
    if (s.state == LinkState.unpaired) {
      device = null;
      slots = const [];
      activeFace = null;
      slotFaces.clear();
    }
    _notify();
    if (s.state == LinkState.connected && !wasConnected) unawaited(refresh());
  }

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    _sub?.cancel();
    super.dispose();
  }

  // ---------------------------------------------------------------- pairing

  Future<LinkStatus> pair(String qr) async {
    final s = await link.pair(qr.trim());
    _onStatus(s);
    return s;
  }

  Future<void> forget() async {
    await link.forget();
    _onStatus(const LinkStatus(LinkState.unpaired));
  }

  // ---------------------------------------------------------------- device

  /// GET_STATE, LIST_SLOTS and the active face. Errors land in [refreshError].
  Future<void> refresh() async {
    if (refreshing) return;
    refreshing = true;
    refreshError = null;
    _notify();
    try {
      device = await bunny.getState();
      slots = await bunny.listSlots();
      await _loadActive();
    } on Object catch (e) {
      refreshError = e is LinkException ? e.code : e.toString();
    } finally {
      refreshing = false;
      _notify();
    }
    if (refreshError == null) unawaited(_loadSlotPreviews());
  }

  /// Reads every used slot we have no (current) copy of. Runs after the
  /// refresh so the Home screen is never kept waiting on it.
  Future<void> _loadSlotPreviews() async {
    _previewsDirty = true;
    if (_loadingPreviews) return;
    _loadingPreviews = true;
    try {
      while (_previewsDirty) {
        _previewsDirty = false;
        slotFaces.removeWhere((slot, _) => !isUsed(slot));
        final active = activeFace, d = device;
        if (active != null && d != null) slotFaces[d.activeSlot] = active;
        for (var i = 0; i < Bunny.slots; i++) {
          if (_disposed || !connected) return;
          if (!isUsed(i)) continue;
          final have = slotFaces[i];
          // The name comes from LIST_SLOTS, so a renamed slot is re-read.
          if (have != null && have.face.name == slotName(i)) continue;
          try {
            final (face, drawing) = await bunny.getSlot(i);
            slotFaces[i] = SavedFace(face, drawing);
            _notify();
          } on Object {
            // Leave this slot as a name-only tile.
          }
        }
      }
    } finally {
      _loadingPreviews = false;
    }
  }

  Future<void> _loadActive() async {
    final d = device;
    if (d == null || !isUsed(d.activeSlot)) {
      activeFace = null;
      return;
    }
    try {
      final (face, drawing) = await bunny.getSlot(d.activeSlot);
      activeFace = SavedFace(face, drawing);
    } on Object {
      activeFace = null;
    }
  }

  bool isUsed(int slot) {
    final d = device;
    if (d != null) return d.usedMask & (1 << slot) != 0;
    return slots.any((s) => s.slot == slot);
  }

  String? slotName(int slot) {
    for (final s in slots) {
      if (s.slot == slot) return s.name;
    }
    return null;
  }

  int? get activeSlot => device?.activeSlot;

  int? get firstEmptySlot {
    for (var i = 0; i < Bunny.slots; i++) {
      if (!isUsed(i)) return i;
    }
    return null;
  }

  Future<void> activate(int slot) async {
    await bunny.activate(slot);
    await refresh();
  }

  Future<void> deleteSlot(int slot) async {
    await bunny.deleteSlot(slot);
    slotFaces.remove(slot);
    await refresh();
  }

  Future<(FaceDef, Drawing?)> loadSlot(int slot) => bunny.getSlot(slot);

  /// "Send to bunny": PUT_SLOT then ACTIVATE.
  Future<void> sendFace(int slot, FaceDef face, Drawing? drawing) async {
    await bunny.putSlot(slot, face, drawing);
    await bunny.activate(slot);
    slotFaces.remove(slot);
    await refresh();
  }

  Future<void> setDnd(bool on) async {
    await link.request(opSetDnd, Uint8List.fromList([on ? 1 : 0]));
    final d = device;
    if (d != null) device = DeviceState(d.activeSlot, d.usedMask, d.notifEnabled, on);
    _notify();
  }

  // ---------------------------------------------------------------- local library

  Future<void> reloadSaved() async {
    try {
      saved = await store.list();
    } on Object {
      saved = const [];
    }
    _notify();
  }

  Future<void> saveLocal(SavedFace face) async {
    await store.save(face);
    await reloadSaved();
  }

  Future<void> deleteLocal(String name) async {
    await store.delete(name);
    await reloadSaved();
  }

  // ---------------------------------------------------------------- notifications

  Future<NotifConfig> loadNotifConfig() async {
    final c = await link.getNotifConfig();
    notifConfig = c;
    _notify();
    return c;
  }

  Future<void> loadClock() async {
    try {
      clockStyle = await link.getClock();
    } on Object {
      // No clock choice yet (or no native side on this platform): keep the face.
    }
    _notify();
  }

  /// Saves the clock; sent to the bunny now when connected, else on connect.
  Future<void> setClock(int style) async {
    final previous = clockStyle;
    clockStyle = style;
    _notify();
    try {
      await link.setClock(style);
    } on Object {
      clockStyle = previous;
      _notify();
      rethrow;
    }
  }

  Future<void> saveNotifConfig(NotifConfig c) async {
    final previous = notifConfig;
    notifConfig = c;
    _notify();
    try {
      await link.setNotifConfig(c);
    } on Object {
      notifConfig = previous;
      _notify();
      rethrow;
    }
  }
}
