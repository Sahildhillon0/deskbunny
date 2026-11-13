// Shared "GIF / image to pixels" flow: pick a file, choose how it becomes
// 1-bit pixels (dither, invert, threshold), decode it. Used by the drawing
// editor and by "Add GIF / images" on Home.
import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';

import '../../app/pixels.dart';
import '../../app/theme.dart';
import 'common.dart';

class ImportOptions {
  final int threshold;
  final bool dither, invert;
  const ImportOptions(this.threshold, this.dither, this.invert);
}

/// Picks a GIF or image, asks how to convert it, and decodes it. null when the
/// user backs out of either step. Throws FormatException for an unreadable file.
Future<ImportedFrames?> pickImportedImage(
  BuildContext context, {
  String note = 'GIFs keep up to 10 frames, evenly spread.',
}) async {
  final file = await openFile(
    acceptedTypeGroups: const [
      XTypeGroup(label: 'Images', extensions: ['gif', 'png', 'jpg', 'jpeg', 'bmp', 'webp'], mimeTypes: ['image/*']),
    ],
  );
  if (file == null || !context.mounted) return null;
  final opts = await showModalBottomSheet<ImportOptions>(
    context: context,
    builder: (_) => ImportSheet(note: note),
  );
  if (opts == null) return null;
  final bytes = await file.readAsBytes();
  return importImage(bytes, threshold: opts.threshold, dither: opts.dither, invert: opts.invert);
}

class ImportSheet extends StatefulWidget {
  final String note;
  const ImportSheet({super.key, required this.note});

  @override
  State<ImportSheet> createState() => _ImportSheetState();
}

class _ImportSheetState extends State<ImportSheet> {
  int _threshold = 128;
  bool _dither = true, _invert = false;

  @override
  Widget build(BuildContext context) => SafeArea(
    child: Padding(
      padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const Text('Import', style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600)),
          const SizedBox(height: 4),
          Text(widget.note, style: const TextStyle(fontSize: 12, color: NixxaColors.text3)),
          SwitchRow(
            label: 'Dither',
            hint: 'Shades become dot patterns',
            value: _dither,
            onChanged: (v) => setState(() => _dither = v),
          ),
          SwitchRow(
            label: 'Invert',
            hint: 'For dark drawings on a light background',
            value: _invert,
            onChanged: (v) => setState(() => _invert = v),
          ),
          SliderRow(
            label: 'Threshold',
            value: _threshold,
            min: 1,
            max: 254,
            onChanged: (v) => setState(() => _threshold = v),
          ),
          const SizedBox(height: 8),
          FilledButton(
            key: const Key('import-confirm'),
            onPressed: () => Navigator.pop(context, ImportOptions(_threshold, _dither, _invert)),
            child: const Text('Import'),
          ),
        ],
      ),
    ),
  );
}
