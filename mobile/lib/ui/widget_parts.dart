// Pieces every widget screen shares: sending its picture to a slot, and the
// "keep it updated and rotate" switch that hands it to the background scheduler.
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../app/theme.dart';
import '../app/widget_settings.dart';
import '../model/drawing.dart';
import '../model/face.dart';
import 'widgets/common.dart';
import 'widgets/oled.dart';
import 'widgets/slot_picker.dart';

String widgetTitle(String id) => switch (id) {
  WidgetIds.weather => 'Weather',
  WidgetIds.air => 'Air quality',
  WidgetIds.battery => 'Phone battery',
  WidgetIds.event => 'Next event',
  WidgetIds.counter => 'Counter',
  WidgetIds.status => 'Status sign',
  WidgetIds.nowPlaying => 'Now playing',
  _ => 'Widget',
};

/// Asks which slot (suggesting the one the widget already uses), warns before
/// taking a slot another widget holds, then returns it. null when cancelled.
Future<int?> chooseWidgetSlot(BuildContext context, String id) async {
  final app = context.read<AppState>();
  final slot = await pickSlot(context, suggested: app.settings.slots[id] ?? app.firstEmptySlot ?? app.activeSlot);
  if (slot == null || !context.mounted) return null;
  final other = app.widgetInSlot(slot);
  if (other != null && other != id) {
    final ok = await confirm(
      context,
      title: 'Replace ${widgetTitle(other)}?',
      body: 'Slot ${slot + 1} holds the ${widgetTitle(other).toLowerCase()} widget. It is taken out of auto-update.',
      action: 'Replace',
      destructive: true,
    );
    if (!ok) return null;
  }
  return slot;
}

/// Puts [frame] in a slot the user picks, shows it, and remembers the slot.
/// True when it was sent.
Future<bool> sendWidget(BuildContext context, String id, Uint8List frame) async {
  final app = context.read<AppState>();
  final slot = await chooseWidgetSlot(context, id);
  if (slot == null || !context.mounted) return false;
  final ok = await guard(
    context,
    () => app.sendFace(
      slot,
      FaceDef(name: WidgetIds.faceName(id)),
      Drawing(mode: DrawingMode.replace, frameMs: 1000, frames: [frame]),
    ),
    success: 'Sent to slot ${slot + 1}',
  );
  if (ok) app.assignSlot(id, slot);
  return ok;
}

/// A picture, or a message standing in for one, in the bezel.
class WidgetPreview extends StatelessWidget {
  final Uint8List? frame;
  final bool loading;
  final String empty;
  const WidgetPreview({super.key, required this.frame, this.loading = false, required this.empty});

  @override
  Widget build(BuildContext context) => OledBezel(
    child: frame != null
        ? OledFrame(frame!)
        : AspectRatio(
            aspectRatio: 2,
            child: Container(
              color: NixxaColors.panel,
              alignment: Alignment.center,
              child: loading
                  ? const CircularProgressIndicator(strokeWidth: 2)
                  : Text(empty, style: const TextStyle(color: NixxaColors.text3)),
            ),
          ),
  );
}

/// Refresh and Send buttons, plus (for widgets that can rotate) the switch
/// that puts the widget in the background rotation.
class WidgetSendPanel extends StatelessWidget {
  final String id;
  final Uint8List? frame;
  final VoidCallback? onRefresh;
  final bool canRotate;

  /// Runs after a successful send (for example, to turn Do Not Disturb on).
  final Future<void> Function()? afterSend;
  const WidgetSendPanel({
    super.key,
    required this.id,
    required this.frame,
    this.onRefresh,
    this.canRotate = true,
    this.afterSend,
  });

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    final s = app.settings;
    final slot = s.slots[id];
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const SizedBox(height: 12),
        Row(
          children: [
            if (onRefresh != null) ...[
              OutlinedButton.icon(
                key: const Key('dw-refresh'),
                onPressed: onRefresh,
                icon: const Icon(Icons.refresh),
                label: const Text('Refresh'),
              ),
              const SizedBox(width: 8),
            ],
            Expanded(
              child: FilledButton.icon(
                key: const Key('dw-send'),
                onPressed: frame != null && app.connected
                    ? () async {
                        final sent = await sendWidget(context, id, frame!);
                        if (sent && afterSend != null) await afterSend!();
                      }
                    : null,
                icon: const Icon(Icons.send_outlined),
                label: Text(app.connected ? 'Send to bunny' : 'Connect to send'),
              ),
            ),
          ],
        ),
        if (canRotate) ...[
          const SizedBox(height: 8),
          SwitchRow(
            key: const Key('dw-rotation'),
            label: 'Keep it updated and rotate',
            hint: slot == null
                ? 'Send it once first, to choose its slot'
                : s.autoOn
                ? 'Slot ${slot + 1}. Part of the auto-update rotation.'
                : 'Slot ${slot + 1}. Turn on auto-update on the Widgets screen to use it.',
            value: s.rotation.contains(id),
            onChanged: slot == null
                ? null
                : (v) => app.updateSettings(
                    (s) => s.copyWith(rotation: v ? {...s.rotation, id} : s.rotation.difference({id})),
                  ),
          ),
        ],
      ],
    );
  }
}
