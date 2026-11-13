// Expressions: every face in one place, the built-in ones and the ones saved on
// this phone. Tap one to open it in the editor, then send it to the bunny.
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../app/labels.dart';
import '../app/theme.dart';
import '../model/drawing.dart';
import '../model/face.dart';
import '../model/presets.dart';
import 'face_editor_screen.dart';
import 'widgets/common.dart';
import 'widgets/oled.dart';

class ExpressionsScreen extends StatelessWidget {
  const ExpressionsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    final slot = app.firstEmptySlot;
    return Scaffold(
      appBar: AppBar(title: const Text('Expressions')),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
        children: [
          if (app.saved.isNotEmpty) ...[
            const SectionLabel('My faces'),
            _grid([
              for (final s in app.saved)
                _Tile(
                  key: Key('expr-saved-${s.face.name}'),
                  face: s.face,
                  drawing: s.drawing,
                  subtitle: s.drawing != null ? 'with drawing' : moodLabel(s.face.mood),
                  onTap: () => _open(context, s.face, s.drawing, null),
                ),
            ]),
          ],
          const SectionLabel('Built in'),
          _grid([
            for (var i = 0; i < presets.length; i++)
              _Tile(
                key: Key('expr-preset-$i'),
                face: presets[i],
                subtitle: moodLabel(presets[i].mood),
                onTap: () => _open(context, presets[i], null, slot),
              ),
          ]),
        ],
      ),
    );
  }

  Future<void> _open(BuildContext context, FaceDef face, Drawing? drawing, int? slot) =>
      Navigator.of(context).push(
        MaterialPageRoute<void>(
          builder: (_) => FaceEditorScreen(face: face, drawing: drawing, slot: slot),
        ),
      );

  Widget _grid(List<Widget> children) => GridView.count(
    crossAxisCount: 2,
    shrinkWrap: true,
    physics: const NeverScrollableScrollPhysics(),
    mainAxisSpacing: 12,
    crossAxisSpacing: 12,
    childAspectRatio: 1.25,
    children: children,
  );
}

class _Tile extends StatelessWidget {
  final FaceDef face;
  final Drawing? drawing;
  final String subtitle;
  final VoidCallback onTap;
  const _Tile({super.key, required this.face, this.drawing, required this.subtitle, required this.onTap});

  @override
  Widget build(BuildContext context) => InkWell(
    borderRadius: BorderRadius.circular(8),
    onTap: onTap,
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        OledThumb(face: face, drawing: drawing),
        const SizedBox(height: 4),
        Text(
          face.name.isEmpty ? 'Unnamed' : face.name,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: const TextStyle(fontSize: 12),
        ),
        Text(
          subtitle,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: const TextStyle(fontSize: 11, color: NixxaColors.text3),
        ),
      ],
    ),
  );
}
