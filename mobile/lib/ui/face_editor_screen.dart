// 4. Face editor: every FaceDef field with a live preview. "Send to bunny" =
// PUT_SLOT + ACTIVATE into a chosen slot.
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../app/face_store.dart';
import '../app/labels.dart';
import '../app/theme.dart';
import '../model/drawing.dart';
import '../model/face.dart';
import 'drawing_editor_screen.dart';
import 'library_screen.dart' show exportFace;
import 'widgets/common.dart';
import 'widgets/oled.dart';
import 'widgets/slot_picker.dart';

class FaceEditorScreen extends StatefulWidget {
  final FaceDef face;
  final Drawing? drawing;

  /// The slot this face came from (suggested when sending), if any.
  final int? slot;
  const FaceEditorScreen({super.key, required this.face, this.drawing, this.slot});

  @override
  State<FaceEditorScreen> createState() => _FaceEditorScreenState();
}

class _FaceEditorScreenState extends State<FaceEditorScreen> {
  late FaceDef _face = widget.face;
  late Drawing? _drawing = widget.drawing;
  late final _name = TextEditingController(text: widget.face.name);
  bool _dirty = false;
  bool _sending = false;

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  void _edit(FaceDef next) => setState(() {
    _face = next;
    _dirty = true;
  });

  SavedFace get _saved => SavedFace(_face, _drawing);

  Future<void> _send() async {
    final app = context.read<AppState>();
    final slot = await pickSlot(context, suggested: widget.slot ?? app.activeSlot ?? app.firstEmptySlot);
    if (slot == null || !mounted) return;
    setState(() => _sending = true);
    final ok = await guard(context, () => app.sendFace(slot, _face, _drawing), success: 'Sent to slot ${slot + 1}');
    if (mounted) setState(() => _sending = false);
    if (ok) _dirty = false;
  }

  Future<void> _saveLocal() async {
    final app = context.read<AppState>();
    final problem = validateFaceName(_face.name);
    if (problem != null) {
      showMessage(context, problem, error: true);
      return;
    }
    final exists = app.saved.any((s) => s.face.name == _face.name);
    if (exists &&
        !await confirm(
          context,
          title: 'Replace ${_face.name}?',
          body: 'A face with this name is already saved on this phone.',
          action: 'Replace',
        )) {
      return;
    }
    if (!mounted) return;
    final ok = await guard(context, () => app.saveLocal(_saved), success: 'Saved ${_face.name} on this phone');
    if (ok) setState(() => _dirty = false);
  }

  Future<void> _editDrawing() async {
    final d = await Navigator.of(context).push<Drawing>(
      MaterialPageRoute(
        builder: (_) => DrawingEditorScreen(drawing: _drawing, face: _face),
      ),
    );
    if (d != null) {
      setState(() {
        _drawing = d;
        _dirty = true;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final connected = context.select<AppState, bool>((a) => a.connected);
    final f = _face;
    return PopScope(
      canPop: !_dirty,
      onPopInvokedWithResult: (didPop, _) async {
        if (didPop) return;
        final nav = Navigator.of(context);
        if (await confirm(
          context,
          title: 'Discard changes?',
          body: 'This face has changes that are not saved or sent.',
          action: 'Discard',
          destructive: true,
        )) {
          _dirty = false;
          nav.pop();
        }
      },
      child: Scaffold(
        appBar: AppBar(
          title: const Text('Face'),
          actions: [
            IconButton(
              key: const Key('editor-save'),
              tooltip: 'Save on phone',
              onPressed: _saveLocal,
              icon: const Icon(Icons.save_outlined),
            ),
            IconButton(
              tooltip: 'Export',
              onPressed: () => guard(context, () => exportFace(_saved)),
              icon: const Icon(Icons.ios_share),
            ),
          ],
        ),
        body: Column(
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 4, 16, 8),
              child: OledBezel(
                child: OledPreview(face: f, drawing: _drawing),
              ),
            ),
            const Divider(),
            Expanded(
              child: ListView(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
                children: [
                  const SectionLabel('Name'),
                  TextField(
                    key: const Key('editor-name'),
                    controller: _name,
                    textCapitalization: TextCapitalization.characters,
                    maxLength: 12,
                    inputFormatters: [_UpperCase(), FilteringTextInputFormatter.allow(RegExp(r'[A-Z0-9 ]'))],
                    decoration: const InputDecoration(counterText: '', hintText: 'A-Z, 0-9 and spaces'),
                    onChanged: (v) => _edit(f.copyWith(name: v)),
                  ),
                  _drawingSection(),
                  const SectionLabel('Mood'),
                  Wrap(
                    spacing: 6,
                    runSpacing: 6,
                    children: [
                      for (final m in Mood.values)
                        ChoiceChip(
                          key: Key('mood-${m.name}'),
                          label: Text(moodLabel(m)),
                          selected: f.mood == m,
                          showCheckmark: false,
                          onSelected: (_) => _edit(f.copyWith(mood: m)),
                        ),
                    ],
                  ),
                  const SectionLabel('Animation'),
                  SegmentedButton<CycleMode>(
                    segments: [for (final c in CycleMode.values) ButtonSegment(value: c, label: Text(cycleLabel(c)))],
                    selected: {f.cycle},
                    showSelectedIcon: false,
                    onSelectionChanged: (s) => _edit(f.copyWith(cycle: s.first)),
                  ),
                  if (f.cycle != CycleMode.off) ..._cycleSection(f),
                  const SectionLabel('Overlay'),
                  Wrap(
                    spacing: 6,
                    runSpacing: 6,
                    children: [
                      for (final o in FaceOverlay.values)
                        ChoiceChip(
                          label: Text(overlayLabel(o)),
                          selected: f.overlay == o,
                          showCheckmark: false,
                          onSelected: (_) => _edit(f.copyWith(overlay: o)),
                        ),
                    ],
                  ),
                  if (f.cycle != CycleMode.off && f.cycleOverlay)
                    const Padding(
                      padding: EdgeInsets.only(top: 6),
                      child: Text(
                        'While cycling, the overlay follows the mood.',
                        style: TextStyle(fontSize: 12, color: NixxaColors.text3),
                      ),
                    ),
                  const SectionLabel('Eyes'),
                  SliderRow(
                    label: 'Width',
                    value: f.eyeW,
                    min: 8,
                    max: 56,
                    format: _px,
                    onChanged: (v) {
                      _edit(f.copyWith(eyeW: v, eyeR: f.eyeR > v ~/ 2 ? v ~/ 2 : f.eyeR));
                    },
                  ),
                  SliderRow(
                    label: 'Height',
                    value: f.eyeH,
                    min: 8,
                    max: 56,
                    format: _px,
                    onChanged: (v) => _edit(f.copyWith(eyeH: v)),
                  ),
                  SliderRow(
                    label: 'Corner radius',
                    value: f.eyeR,
                    min: 0,
                    max: f.eyeW ~/ 2,
                    format: _px,
                    onChanged: (v) => _edit(f.copyWith(eyeR: v)),
                  ),
                  SliderRow(
                    label: 'Gap',
                    value: f.eyeGap,
                    min: 0,
                    max: 40,
                    format: _px,
                    onChanged: (v) => _edit(f.copyWith(eyeGap: v)),
                  ),
                  SliderRow(
                    label: 'Vertical offset',
                    value: f.eyeY,
                    min: -20,
                    max: 20,
                    format: (v) => '${v > 0 ? '+' : ''}$v px',
                    onChanged: (v) => _edit(f.copyWith(eyeY: v)),
                  ),
                  SwitchRow(
                    label: 'Cyclops',
                    hint: 'One eye',
                    value: f.cyclops,
                    onChanged: (v) => _edit(f.copyWith(cyclops: v)),
                  ),
                  SwitchRow(
                    label: 'Mouth',
                    value: f.mouth,
                    onChanged: (v) => _edit(f.copyWith(mouth: v)),
                  ),
                  const SectionLabel('Behaviour'),
                  SwitchRow(
                    label: 'Blink',
                    value: f.blink,
                    onChanged: (v) => _edit(f.copyWith(blink: v)),
                  ),
                  if (f.blink) ...[
                    SliderRow(
                      label: 'Blink every',
                      value: f.blinkMin,
                      min: 5,
                      max: 200,
                      format: _secs,
                      onChanged: (v) {
                        _edit(f.copyWith(blinkMin: v, blinkMax: f.blinkMax < v ? v : f.blinkMax));
                      },
                    ),
                    SliderRow(
                      label: 'to',
                      value: f.blinkMax,
                      min: f.blinkMin,
                      max: 200,
                      format: _secs,
                      onChanged: (v) => _edit(f.copyWith(blinkMax: v)),
                    ),
                  ],
                  SwitchRow(
                    label: 'Watching',
                    hint: 'Glance around now and then',
                    value: f.lookAround,
                    onChanged: (v) => _edit(f.copyWith(lookAround: v)),
                  ),
                  if (f.lookAround) ...[
                    SliderRow(
                      label: 'Glance every',
                      value: f.lookMin,
                      min: 5,
                      max: 200,
                      format: _secs,
                      onChanged: (v) {
                        _edit(f.copyWith(lookMin: v, lookMax: f.lookMax < v ? v : f.lookMax));
                      },
                    ),
                    SliderRow(
                      label: 'to',
                      value: f.lookMax,
                      min: f.lookMin,
                      max: 200,
                      format: _secs,
                      onChanged: (v) => _edit(f.copyWith(lookMax: v)),
                    ),
                    SliderRow(
                      label: 'Glance X',
                      value: f.lookX,
                      min: 0,
                      max: 40,
                      format: _px,
                      onChanged: (v) => _edit(f.copyWith(lookX: v)),
                    ),
                    SliderRow(
                      label: 'Glance Y',
                      value: f.lookY,
                      min: 0,
                      max: 20,
                      format: _px,
                      onChanged: (v) => _edit(f.copyWith(lookY: v)),
                    ),
                  ],
                  SwitchRow(
                    label: 'Breathe',
                    value: f.breathe,
                    onChanged: (v) => _edit(f.copyWith(breathe: v)),
                  ),
                  SliderRow(
                    label: 'Speed',
                    value: f.speed,
                    min: 1,
                    max: 10,
                    format: (v) => '$v / 10',
                    onChanged: (v) => _edit(f.copyWith(speed: v)),
                  ),
                  const SectionLabel('Display'),
                  SliderRow(
                    label: 'Contrast',
                    value: f.contrast,
                    min: 0,
                    max: 255,
                    format: (v) => '0x${v.toRadixString(16).toUpperCase().padLeft(2, '0')}',
                    onChanged: (v) => _edit(f.copyWith(contrast: v)),
                  ),
                  SwitchRow(
                    label: 'Invert',
                    value: f.invert,
                    onChanged: (v) => _edit(f.copyWith(invert: v)),
                  ),
                  SwitchRow(
                    label: 'Rotate 180°',
                    hint: 'For a module mounted upside down',
                    value: f.rotate180,
                    onChanged: (v) => _edit(f.copyWith(rotate180: v)),
                  ),
                  SliderRow(
                    label: 'Random seed',
                    value: f.seed,
                    min: 0,
                    max: 255,
                    onChanged: (v) => _edit(f.copyWith(seed: v)),
                  ),
                ],
              ),
            ),
          ],
        ),
        bottomNavigationBar: SafeArea(
          child: Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 8),
            child: FilledButton.icon(
              key: const Key('editor-send'),
              onPressed: connected && !_sending ? _send : null,
              icon: _sending
                  ? const SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(strokeWidth: 2, color: NixxaColors.accentInk),
                    )
                  : const Icon(Icons.send),
              label: Text(connected ? 'Send to bunny' : 'Bunny not connected'),
            ),
          ),
        ),
      ),
    );
  }

  Widget _drawingSection() {
    final d = _drawing;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const SectionLabel('Drawing'),
        if (d == null)
          OutlinedButton.icon(
            onPressed: _editDrawing,
            icon: const Icon(Icons.brush_outlined),
            label: const Text('Add a drawing'),
          )
        else
          Row(
            children: [
              SizedBox(
                width: 96,
                child: OledThumb(face: _face, drawing: d),
              ),
              const SizedBox(width: 12),
              Expanded(
                child: Text(
                  '${d.frames.length} frame${d.frames.length == 1 ? '' : 's'} · ${d.mode == DrawingMode.replace ? 'replaces the face' : 'over the face'}',
                  style: const TextStyle(fontSize: 13, color: NixxaColors.text2),
                ),
              ),
              IconButton(tooltip: 'Edit drawing', onPressed: _editDrawing, icon: const Icon(Icons.edit_outlined)),
              IconButton(
                tooltip: 'Remove drawing',
                onPressed: () => setState(() {
                  _drawing = null;
                  _dirty = true;
                }),
                icon: const Icon(Icons.delete_outline),
              ),
            ],
          ),
      ],
    );
  }

  List<Widget> _cycleSection(FaceDef f) {
    final list = f.cycleMoods;
    final full = list.length >= cycleMax;
    return [
      const SizedBox(height: 12),
      Text(
        list.isEmpty
            ? 'Cycling through all 30 moods. Pick up to 12 to limit it.'
            : '${list.length} of $cycleMax moods, in this order',
        style: const TextStyle(fontSize: 12, color: NixxaColors.text3),
      ),
      const SizedBox(height: 8),
      Wrap(
        spacing: 6,
        runSpacing: 6,
        children: [
          for (final m in Mood.values)
            FilterChip(
              key: Key('cycle-${m.name}'),
              label: Text(list.contains(m) ? '${list.indexOf(m) + 1}. ${moodLabel(m)}' : moodLabel(m)),
              selected: list.contains(m),
              showCheckmark: false,
              onSelected: !list.contains(m) && full
                  ? null
                  : (on) => _edit(f.copyWith(cycleMoods: on ? [...list, m] : [...list.where((x) => x != m)])),
            ),
        ],
      ),
      if (list.isNotEmpty)
        Align(
          alignment: Alignment.centerLeft,
          child: TextButton(
            onPressed: () => _edit(f.copyWith(cycleMoods: const [])),
            child: const Text('Use all moods'),
          ),
        ),
      SliderRow(
        label: 'Change every',
        value: f.cycleSeconds,
        min: 1,
        max: 255,
        format: (v) => '$v s',
        onChanged: (v) => _edit(f.copyWith(cycleSeconds: v)),
      ),
      SwitchRow(
        label: 'Overlay follows mood',
        hint: 'Hearts for love, Zzz for sleepy, and so on',
        value: f.cycleOverlay,
        onChanged: (v) => _edit(f.copyWith(cycleOverlay: v)),
      ),
    ];
  }
}

String _px(int v) => '$v px';
String _secs(int v) => '${(v / 10).toStringAsFixed(v % 10 == 0 ? 0 : 1)} s';

class _UpperCase extends TextInputFormatter {
  @override
  TextEditingValue formatEditUpdate(TextEditingValue oldValue, TextEditingValue newValue) =>
      newValue.copyWith(text: newValue.text.toUpperCase());
}
