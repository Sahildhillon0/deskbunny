// 3. Library: the 16 slots on the bunny (tap to show), and faces saved on this
// phone. Import and export of .deskbunny.json.
import 'dart:convert';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:share_plus/share_plus.dart';

import '../app/app_state.dart';
import '../app/face_store.dart';
import '../app/theme.dart';
import '../protocol/bunny.dart';
import 'face_editor_screen.dart';
import 'widgets/common.dart';
import 'widgets/oled.dart';
import 'widgets/preset_picker.dart';
import 'widgets/slot_picker.dart';

Future<void> exportFace(SavedFace face) async {
  final name = faceFileName(face.face);
  final bytes = utf8.encode(faceFileText(face));
  await SharePlus.instance.share(
    ShareParams(
      files: [XFile.fromData(bytes, mimeType: 'application/json', name: name)],
      fileNameOverrides: [name],
      subject: name,
    ),
  );
}

Future<SavedFace?> importFaceFile() async {
  final file = await openFile(
    acceptedTypeGroups: const [
      XTypeGroup(
        label: 'DeskBunny face',
        extensions: ['json'],
        mimeTypes: ['application/json', 'text/json', 'text/plain', 'application/octet-stream'],
      ),
    ],
  );
  if (file == null) return null;
  final bytes = await file.readAsBytes();
  if (bytes.length > 512 * 1024) throw const FormatException('That file is too big to be a face.');
  return parseFaceFile(utf8.decode(bytes, allowMalformed: true));
}

class LibraryScreen extends StatelessWidget {
  const LibraryScreen({super.key});

  Future<void> _import(BuildContext context) async {
    final app = context.read<AppState>();
    await guard(context, () async {
      final face = await importFaceFile();
      if (face == null) return;
      await app.saveLocal(face);
      if (context.mounted) showMessage(context, 'Imported ${face.face.name}');
    });
  }

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    return Scaffold(
      appBar: AppBar(
        title: const Text('Library'),
        actions: [
          IconButton(
            tooltip: 'Import a face file',
            onPressed: () => _import(context),
            icon: const Icon(Icons.file_open_outlined),
          ),
          IconButton(
            tooltip: 'New face',
            onPressed: () => startNewFace(context, slot: app.firstEmptySlot),
            icon: const Icon(Icons.add),
          ),
        ],
      ),
      body: RefreshIndicator(
        onRefresh: () async {
          await app.reloadSaved();
          if (app.connected) await app.refresh();
        },
        child: ListView(
          padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
          children: [
            SectionLabel(
              'On the bunny',
              trailing: app.refreshing
                  ? const SizedBox(width: 12, height: 12, child: CircularProgressIndicator(strokeWidth: 1.5))
                  : null,
            ),
            if (!app.connected)
              const Padding(
                padding: EdgeInsets.only(bottom: 8),
                child: Text(
                  'Connect to the bunny to see and change its slots.',
                  style: TextStyle(fontSize: 12, color: NixxaColors.text3),
                ),
              ),
            GridView.count(
              crossAxisCount: 4,
              shrinkWrap: true,
              physics: const NeverScrollableScrollPhysics(),
              mainAxisSpacing: 8,
              crossAxisSpacing: 8,
              childAspectRatio: 1,
              children: [for (var i = 0; i < Bunny.slots; i++) _SlotCell(slot: i)],
            ),
            const SizedBox(height: 4),
            const Text(
              'Tap a face to show it. Long-press for more.',
              style: TextStyle(fontSize: 12, color: NixxaColors.text3),
            ),
            const SectionLabel('On this phone'),
            if (app.saved.isEmpty)
              const Padding(
                padding: EdgeInsets.symmetric(vertical: 12),
                child: Text(
                  'No saved faces yet. Save one from the editor or import a .deskbunny.json file.',
                  style: TextStyle(color: NixxaColors.text3, fontSize: 13),
                ),
              ),
            for (final s in app.saved) _SavedTile(saved: s),
          ],
        ),
      ),
    );
  }
}

class _SlotCell extends StatelessWidget {
  final int slot;
  const _SlotCell({required this.slot});

  Future<void> _menu(BuildContext context) async {
    final app = context.read<AppState>();
    final used = app.isUsed(slot);
    final action = await showModalBottomSheet<String>(
      context: context,
      builder: (ctx) => SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            ListTile(title: Text('Slot ${slot + 1}${used ? ' · ${app.slotName(slot) ?? ''}' : ' · empty'}')),
            if (used) ...[
              ListTile(
                leading: const Icon(Icons.play_arrow),
                title: const Text('Show it'),
                onTap: () => Navigator.pop(ctx, 'activate'),
              ),
              ListTile(
                leading: const Icon(Icons.edit_outlined),
                title: const Text('Edit'),
                onTap: () => Navigator.pop(ctx, 'edit'),
              ),
              ListTile(
                leading: const Icon(Icons.save_outlined),
                title: const Text('Save on phone'),
                onTap: () => Navigator.pop(ctx, 'save'),
              ),
              ListTile(
                leading: const Icon(Icons.delete_outline, color: NixxaColors.danger),
                title: const Text('Delete from bunny'),
                enabled: app.activeSlot != slot,
                subtitle: app.activeSlot == slot ? const Text('Show another face first') : null,
                onTap: () => Navigator.pop(ctx, 'delete'),
              ),
            ] else
              ListTile(
                leading: const Icon(Icons.add),
                title: const Text('New face here'),
                onTap: () => Navigator.pop(ctx, 'new'),
              ),
          ],
        ),
      ),
    );
    if (action == null || !context.mounted) return;
    switch (action) {
      case 'activate':
        await guard(context, () => app.activate(slot));
      case 'edit':
        await _edit(context);
      case 'save':
        await guard(context, () async {
          final (face, drawing) = await app.loadSlot(slot);
          await app.saveLocal(SavedFace(face, drawing));
        }, success: 'Saved on this phone');
      case 'delete':
        if (await confirm(
          context,
          title: 'Delete slot ${slot + 1}?',
          body: 'The face is removed from the bunny.',
          action: 'Delete',
          destructive: true,
        )) {
          if (context.mounted) await guard(context, () => app.deleteSlot(slot));
        }
      case 'new':
        await startNewFace(context, slot: slot);
    }
  }

  Future<void> _edit(BuildContext context) async {
    final app = context.read<AppState>();
    final nav = Navigator.of(context);
    await guard(context, () async {
      final (face, drawing) = await app.loadSlot(slot);
      await nav.push(
        MaterialPageRoute(
          builder: (_) => FaceEditorScreen(face: face, drawing: drawing, slot: slot),
        ),
      );
    });
  }

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    final used = app.isUsed(slot);
    final active = app.activeSlot == slot;
    final enabled = app.connected;
    return Material(
      color: active ? NixxaColors.accentSoft : NixxaColors.surface2,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(8),
        side: BorderSide(color: active ? NixxaColors.accentLine : NixxaColors.line2),
      ),
      child: InkWell(
        key: Key('slot-$slot'),
        borderRadius: BorderRadius.circular(8),
        onTap: !enabled
            ? null
            : used
            ? (active ? () => _menu(context) : () => guard(context, () => app.activate(slot)))
            : () => _menu(context),
        onLongPress: enabled ? () => _menu(context) : null,
        child: Padding(
          padding: const EdgeInsets.all(6),
          child: FittedBox(
            fit: BoxFit.scaleDown,
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                Text(
                  '${slot + 1}',
                  style: TextStyle(fontSize: 11, color: active ? NixxaColors.accent : NixxaColors.text3),
                ),
                const SizedBox(height: 4),
                Text(
                  used ? (app.slotName(slot) ?? '') : '+',
                  maxLines: 2,
                  textAlign: TextAlign.center,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(fontSize: used ? 11 : 18, color: used ? NixxaColors.text : NixxaColors.text3),
                ),
                if (active) const Text('showing', style: TextStyle(fontSize: 9, color: NixxaColors.ok)),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _SavedTile extends StatelessWidget {
  final SavedFace saved;
  const _SavedTile({required this.saved});

  Future<void> _send(BuildContext context) async {
    final app = context.read<AppState>();
    final slot = await pickSlot(context, suggested: app.firstEmptySlot ?? app.activeSlot);
    if (slot == null || !context.mounted) return;
    await guard(context, () => app.sendFace(slot, saved.face, saved.drawing), success: 'Sent to slot ${slot + 1}');
  }

  @override
  Widget build(BuildContext context) {
    final app = context.read<AppState>();
    final name = saved.face.name;
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: Material(
        color: NixxaColors.surface,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(10),
          side: const BorderSide(color: NixxaColors.line),
        ),
        child: InkWell(
          key: Key('saved-$name'),
          borderRadius: BorderRadius.circular(10),
          onTap: () => Navigator.of(context).push(
            MaterialPageRoute(
              builder: (_) => FaceEditorScreen(face: saved.face, drawing: saved.drawing),
            ),
          ),
          child: Padding(
            padding: const EdgeInsets.all(8),
            child: Row(
              children: [
                SizedBox(
                  width: 88,
                  child: OledThumb(face: saved.face, drawing: saved.drawing),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(name.isEmpty ? 'Unnamed' : name, style: const TextStyle(fontWeight: FontWeight.w600)),
                      Text(
                        saved.drawing != null ? 'with drawing' : 'face',
                        style: const TextStyle(fontSize: 12, color: NixxaColors.text3),
                      ),
                    ],
                  ),
                ),
                IconButton(
                  key: Key('saved-send-$name'),
                  tooltip: 'Send to bunny',
                  onPressed: app.connected ? () => _send(context) : null,
                  icon: const Icon(Icons.send_outlined),
                ),
                PopupMenuButton<String>(
                  onSelected: (v) async {
                    switch (v) {
                      case 'export':
                        await guard(context, () => exportFace(saved));
                      case 'delete':
                        if (await confirm(
                          context,
                          title: 'Delete $name?',
                          body: 'It is removed from this phone.',
                          action: 'Delete',
                          destructive: true,
                        )) {
                          if (context.mounted) await guard(context, () => app.deleteLocal(name));
                        }
                    }
                  },
                  itemBuilder: (_) => const [
                    PopupMenuItem(value: 'export', child: Text('Export')),
                    PopupMenuItem(value: 'delete', child: Text('Delete')),
                  ],
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
