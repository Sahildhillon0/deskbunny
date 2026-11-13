// 2. Home: link state (tap for the Device screen), the active face live, and
// the ways to make a face: Expressions, Widgets, GIF / images, Drawing.
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../app/labels.dart';
import '../app/pixels.dart' show ImportedFrames;
import '../app/theme.dart';
import '../link/device_link.dart';
import '../model/drawing.dart';
import '../model/face.dart';
import 'device_screen.dart';
import 'drawing_editor_screen.dart';
import 'expressions_screen.dart';
import 'face_editor_screen.dart';
import 'widgets/common.dart';
import 'widgets/import_sheet.dart';
import 'widgets/oled.dart';
import 'widgets_screen.dart';

Color linkColor(LinkState s) => switch (s) {
  LinkState.connected => NixxaColors.ok,
  LinkState.connecting || LinkState.disconnected => NixxaColors.warn,
  _ => NixxaColors.danger,
};

class HomeScreen extends StatelessWidget {
  const HomeScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    final s = app.status;
    final active = app.activeFace;
    final slot = app.activeSlot;
    return Scaffold(
      appBar: AppBar(
        title: const Text('nixxa'),
        actions: [
          IconButton(
            tooltip: 'Refresh',
            onPressed: app.connected && !app.refreshing ? app.refresh : null,
            icon: const Icon(Icons.refresh),
          ),
        ],
      ),
      body: RefreshIndicator(
        onRefresh: app.connected ? app.refresh : () async {},
        child: ListView(
          padding: const EdgeInsets.fromLTRB(16, 4, 16, 24),
          children: [
            InkWell(
              key: const Key('home-device'),
              borderRadius: BorderRadius.circular(14),
              onTap: () => Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => const DeviceScreen())),
              child: Panel(
                child: Row(
                  children: [
                    StatusDot(linkColor(s.state)),
                    const SizedBox(width: 10),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            linkStateLabel(s.state),
                            key: const Key('home-status'),
                            style: const TextStyle(fontWeight: FontWeight.w600),
                          ),
                          const SizedBox(height: 2),
                          Text(
                            [
                              if (s.deviceId != null) 'NIXXA-${s.deviceId}',
                              if (s.chip != null) chipLabel(s.chip),
                              if (s.fwVersion != null) 'fw ${s.fwVersion}',
                            ].join(' · '),
                            style: const TextStyle(fontSize: 12, color: NixxaColors.text2),
                          ),
                        ],
                      ),
                    ),
                    if (app.refreshing)
                      const Padding(
                        padding: EdgeInsets.only(right: 8),
                        child: SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2)),
                      ),
                    const Icon(Icons.chevron_right, color: NixxaColors.text3),
                  ],
                ),
              ),
            ),
            if (s.state == LinkState.disconnected || s.state == LinkState.bluetoothOff) ...[
              const SizedBox(height: 8),
              Text(
                s.state == LinkState.bluetoothOff
                    ? 'Turn Bluetooth on. nixxa reconnects by itself.'
                    : 'nixxa reconnects by itself when the bunny is powered and in range.',
                style: const TextStyle(fontSize: 12, color: NixxaColors.text3),
              ),
            ],
            const SectionLabel('On the bunny now'),
            OledBezel(
              child: active != null
                  ? OledPreview(face: active.face, drawing: active.drawing)
                  : AspectRatio(
                      aspectRatio: 2,
                      child: Container(
                        color: NixxaColors.panel,
                        alignment: Alignment.center,
                        child: Text(
                          app.connected ? (app.refreshing ? 'Loading…' : 'No face loaded') : 'Not connected',
                          style: const TextStyle(color: NixxaColors.text3),
                        ),
                      ),
                    ),
            ),
            if (active != null)
              Padding(
                padding: const EdgeInsets.only(top: 4),
                child: Row(
                  children: [
                    Expanded(
                      child: Text(
                        '${active.face.name.isEmpty ? 'Unnamed' : active.face.name}  ·  slot ${(slot ?? 0) + 1}'
                        '${active.drawing != null ? '  ·  drawing' : ''}',
                        style: const TextStyle(color: NixxaColors.text2, fontSize: 13),
                      ),
                    ),
                    TextButton.icon(
                      key: const Key('home-edit'),
                      onPressed: () => Navigator.of(context).push(
                        MaterialPageRoute<void>(
                          builder: (_) => FaceEditorScreen(face: active.face, drawing: active.drawing, slot: slot),
                        ),
                      ),
                      icon: const Icon(Icons.edit_outlined, size: 16),
                      label: const Text('Edit'),
                    ),
                  ],
                ),
              ),
            const SectionLabel('Faces'),
            _NavRow(
              key: const Key('home-expressions'),
              icon: Icons.emoji_emotions_outlined,
              title: 'Expressions',
              subtitle: 'Every face, ready to edit and send',
              onTap: () => Navigator.of(context).push(
                MaterialPageRoute<void>(builder: (_) => const ExpressionsScreen()),
              ),
            ),
            _NavRow(
              key: const Key('home-widgets'),
              icon: Icons.widgets_outlined,
              title: 'Widgets',
              subtitle: 'Clocks, weather, air quality',
              onTap: () => Navigator.of(context).push(
                MaterialPageRoute<void>(builder: (_) => const WidgetsScreen()),
              ),
            ),
            _NavRow(
              key: const Key('home-images'),
              icon: Icons.gif_box_outlined,
              title: 'Add GIF / images',
              subtitle: 'Turn a picture or a GIF into a face',
              onTap: () => _addImage(context),
            ),
            _NavRow(
              key: const Key('home-drawing'),
              icon: Icons.brush_outlined,
              title: 'Drawing',
              subtitle: 'Draw pixel by pixel and add text',
              onTap: () => _newDrawing(context),
            ),
          ],
        ),
      ),
    );
  }

  Future<void> _addImage(BuildContext context) async {
    final nav = Navigator.of(context);
    final slot = context.read<AppState>().firstEmptySlot;
    final ImportedFrames r;
    try {
      final got = await pickImportedImage(context);
      if (got == null) return;
      r = got;
    } on Object catch (e) {
      if (context.mounted) showMessage(context, errorMessage(e), error: true);
      return;
    }
    if (context.mounted && r.sourceFrames > r.frames.length) {
      showMessage(context, 'Kept ${r.frames.length} of ${r.sourceFrames} frames (the bunny shows up to 10).');
    }
    await nav.push(
      MaterialPageRoute<void>(
        builder: (_) => FaceEditorScreen(
          face: FaceDef(name: r.sourceFrames > 1 ? 'GIF' : 'IMAGE'),
          drawing: Drawing(mode: DrawingMode.replace, frameMs: r.frameMs, frames: r.frames),
          slot: slot,
        ),
      ),
    );
  }

  Future<void> _newDrawing(BuildContext context) async {
    final nav = Navigator.of(context);
    final slot = context.read<AppState>().firstEmptySlot;
    final drawing = await nav.push(MaterialPageRoute(builder: (_) => const DrawingEditorScreen()));
    if (drawing == null) return;
    await nav.push(
      MaterialPageRoute(
        builder: (_) => FaceEditorScreen(
          face: const FaceDef(name: 'DRAWING'),
          drawing: drawing,
          slot: slot,
        ),
      ),
    );
  }
}

class _NavRow extends StatelessWidget {
  final IconData icon;
  final String title, subtitle;
  final VoidCallback onTap;
  const _NavRow({super.key, required this.icon, required this.title, required this.subtitle, required this.onTap});

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(bottom: 8),
    child: Material(
      color: NixxaColors.surface2,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: const BorderSide(color: NixxaColors.line2),
      ),
      child: InkWell(
        borderRadius: BorderRadius.circular(12),
        onTap: onTap,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 14),
          child: Row(
            children: [
              Icon(icon, size: 22, color: NixxaColors.accent),
              const SizedBox(width: 14),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(title, style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600)),
                    const SizedBox(height: 2),
                    Text(subtitle, style: const TextStyle(fontSize: 12, color: NixxaColors.text3)),
                  ],
                ),
              ),
              const Icon(Icons.chevron_right, color: NixxaColors.text3),
            ],
          ),
        ),
      ),
    ),
  );
}
