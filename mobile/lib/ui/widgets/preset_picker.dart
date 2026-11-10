// "New face": pick a built-in preset to start from (same list as the desktop).
import 'package:flutter/material.dart';

import '../../app/labels.dart';
import '../../app/theme.dart';
import '../../model/face.dart';
import '../../model/presets.dart';
import '../face_editor_screen.dart';
import 'oled.dart';

Future<void> startNewFace(BuildContext context, {int? slot}) async {
  final nav = Navigator.of(context);
  final face = await showModalBottomSheet<FaceDef>(
    context: context,
    isScrollControlled: true,
    builder: (ctx) => DraggableScrollableSheet(
      expand: false,
      initialChildSize: 0.7,
      maxChildSize: 0.95,
      builder: (ctx, scroll) => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const Padding(
            padding: EdgeInsets.fromLTRB(16, 0, 16, 8),
            child: Text('Start from', style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600)),
          ),
          Expanded(
            child: GridView.builder(
              controller: scroll,
              padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
              gridDelegate: const SliverGridDelegateWithFixedCrossAxisCount(
                crossAxisCount: 2,
                mainAxisSpacing: 10,
                crossAxisSpacing: 10,
                childAspectRatio: 1.2,
              ),
              itemCount: presets.length,
              itemBuilder: (ctx, i) {
                final p = presets[i];
                return InkWell(
                  key: Key('preset-$i'),
                  borderRadius: BorderRadius.circular(8),
                  onTap: () => Navigator.pop(ctx, p),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      OledThumb(face: p),
                      const SizedBox(height: 4),
                      Text(p.name, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 12)),
                      Text(
                        moodLabel(p.mood),
                        maxLines: 1,
                        style: const TextStyle(fontSize: 11, color: NixxaColors.text3),
                      ),
                    ],
                  ),
                );
              },
            ),
          ),
        ],
      ),
    ),
  );
  if (face == null) return;
  await nav.push(
    MaterialPageRoute<void>(
      builder: (_) => FaceEditorScreen(face: face, slot: slot),
    ),
  );
}
