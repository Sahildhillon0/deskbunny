// 2. Home: link state, chip and firmware, the active face live, quick actions.
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../app/labels.dart';
import '../app/theme.dart';
import '../link/device_link.dart';
import '../model/face.dart';
import 'drawing_editor_screen.dart';
import 'face_editor_screen.dart';
import 'widgets/common.dart';
import 'widgets/oled.dart';
import 'widgets/preset_picker.dart';

Color linkColor(LinkState s) => switch (s) {
  LinkState.connected => NixxaColors.ok,
  LinkState.connecting || LinkState.disconnected => NixxaColors.warn,
  _ => NixxaColors.danger,
};

class HomeScreen extends StatelessWidget {
  final VoidCallback? onOpenLibrary;
  const HomeScreen({super.key, this.onOpenLibrary});

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
            Panel(
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
                    const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2)),
                ],
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
            const SizedBox(height: 8),
            if (active != null)
              Text(
                '${active.face.name.isEmpty ? 'Unnamed' : active.face.name}  ·  slot ${(slot ?? 0) + 1}'
                '${active.drawing != null ? '  ·  drawing' : ''}',
                style: const TextStyle(color: NixxaColors.text2, fontSize: 13),
              ),
            const SectionLabel('Quick actions'),
            GridView.count(
              crossAxisCount: 2,
              shrinkWrap: true,
              physics: const NeverScrollableScrollPhysics(),
              mainAxisSpacing: 8,
              crossAxisSpacing: 8,
              childAspectRatio: 2.6,
              children: [
                _Action(
                  icon: Icons.edit_outlined,
                  label: 'Edit this face',
                  onTap: active == null
                      ? null
                      : () => Navigator.of(context).push(
                          MaterialPageRoute(
                            builder: (_) => FaceEditorScreen(face: active.face, drawing: active.drawing, slot: slot),
                          ),
                        ),
                ),
                _Action(
                  icon: Icons.add,
                  label: 'New face',
                  onTap: () => startNewFace(context, slot: app.firstEmptySlot),
                ),
                _Action(icon: Icons.brush_outlined, label: 'New drawing', onTap: () => _newDrawing(context)),
                _Action(icon: Icons.grid_view, label: 'Library', onTap: onOpenLibrary),
                _Action(
                  icon: app.device?.dnd == true ? Icons.do_not_disturb_on : Icons.do_not_disturb_off_outlined,
                  label: app.device?.dnd == true ? 'DND is on' : 'DND is off',
                  onTap: app.connected && app.device != null
                      ? () => guard(context, () => app.setDnd(!(app.device?.dnd ?? false)))
                      : null,
                ),
                _Action(
                  icon: Icons.notifications_active_outlined,
                  label: 'Test notification',
                  onTap: app.connected
                      ? () => guard(context, app.link.sendTestNotification, success: 'Sent a test notification')
                      : null,
                ),
              ],
            ),
          ],
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

class _Action extends StatelessWidget {
  final IconData icon;
  final String label;
  final VoidCallback? onTap;
  const _Action({required this.icon, required this.label, this.onTap});

  @override
  Widget build(BuildContext context) {
    final enabled = onTap != null;
    return Material(
      color: NixxaColors.surface2,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(10),
        side: const BorderSide(color: NixxaColors.line2),
      ),
      child: InkWell(
        borderRadius: BorderRadius.circular(10),
        onTap: onTap,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12),
          child: Row(
            children: [
              Icon(icon, size: 20, color: enabled ? NixxaColors.accent : NixxaColors.text3),
              const SizedBox(width: 10),
              Expanded(
                child: Text(
                  label,
                  maxLines: 2,
                  style: TextStyle(fontSize: 13, color: enabled ? NixxaColors.text : NixxaColors.text3),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
