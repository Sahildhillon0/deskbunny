// Clock picker: the bunny can show a clock instead of its face. Tiles are still
// previews at 10:08:30 (drawn by the firmware's clock module through the C
// engine); the chosen one is saved on the phone and sent with SET_CLOCK.
// Reached from the Device screen, like the other sub-screens.
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../app/pixels.dart';
import '../app/theme.dart';
import '../model/face.dart';
import '../protocol/bunny.dart';
import 'widgets/common.dart';
import 'widgets/oled.dart';

/// The time every clock tile is drawn at.
const int clockPreviewHour = 10;
const int clockPreviewMinute = 8;
const int clockPreviewSecond = 30;

T _orElse<T>(T Function() f, T Function() fallback) {
  try {
    return f();
  } on Object {
    return fallback();
  }
}

class ClockScreen extends StatefulWidget {
  const ClockScreen({super.key});

  @override
  State<ClockScreen> createState() => _ClockScreenState();
}

class _ClockScreenState extends State<ClockScreen> {
  // Index i = clock style i + 1. Rendered once: the clocks are not animated.
  late final List<Uint8List> _frames;
  late final List<String> _names;

  @override
  void initState() {
    super.initState();
    final r = context.read<AppState>().clockRenderer;
    _frames = [
      for (var s = 1; s <= Bunny.clockStyles; s++)
        _orElse(
          () => r.frame(s, clockPreviewHour, clockPreviewMinute, clockPreviewSecond),
          blankFrame,
        ),
    ];
    _names = [
      for (var s = 1; s <= Bunny.clockStyles; s++) _orElse(() => r.name(s), () => 'Clock $s'),
    ];
  }

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    final chosen = app.clockStyle;
    final tiles = <Widget>[
      _tile(
        0,
        'Face',
        OledThumb(face: app.activeFace?.face ?? const FaceDef(), drawing: app.activeFace?.drawing),
        selected: chosen == 0,
      ),
      for (var s = 1; s <= Bunny.clockStyles; s++)
        _tile(s, _names[s - 1], OledFrame(_frames[s - 1]), selected: chosen == s),
    ];
    return Scaffold(
      appBar: AppBar(title: const Text('Clock')),
      body: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 4, 16, 8),
            child: Text(
              app.connected
                  ? 'Tap a tile to show it on the bunny. Face means the bunny shows its own face.'
                  : 'The bunny is not connected. Your choice is saved and sent when it connects.',
              key: const Key('clock-status'),
              style: const TextStyle(fontSize: 12, color: NixxaColors.text2, height: 1.4),
            ),
          ),
          Expanded(
            child: GridView.count(
              padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
              crossAxisCount: 2,
              mainAxisSpacing: 10,
              crossAxisSpacing: 10,
              childAspectRatio: 1.15,
              children: tiles,
            ),
          ),
        ],
      ),
    );
  }

  Widget _tile(int style, String name, Widget preview, {required bool selected}) {
    return Material(
      key: Key('clock-tile-$style'),
      color: selected ? NixxaColors.accentSoft : NixxaColors.surface,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(10),
        side: BorderSide(color: selected ? NixxaColors.accentLine : NixxaColors.line),
      ),
      child: InkWell(
        borderRadius: BorderRadius.circular(10),
        onTap: () => guard(context, () => context.read<AppState>().setClock(style)),
        child: Semantics(
          selected: selected,
          button: true,
          label: name,
          child: Padding(
            padding: const EdgeInsets.all(8),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                ClipRRect(borderRadius: BorderRadius.circular(4), child: preview),
                const SizedBox(height: 6),
                Text(
                  name,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 13,
                    fontWeight: selected ? FontWeight.w600 : FontWeight.w400,
                    color: selected ? NixxaColors.text : NixxaColors.text2,
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
