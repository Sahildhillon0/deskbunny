// Shared controls in the desktop style: uppercase section labels, labelled
// sliders and switches, error snackbars and confirm dialogs.
import 'package:flutter/material.dart';

import '../../app/labels.dart';
import '../../app/theme.dart';

class SectionLabel extends StatelessWidget {
  final String text;
  final Widget? trailing;
  const SectionLabel(this.text, {super.key, this.trailing});

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.fromLTRB(0, 20, 0, 8),
    child: Row(
      children: [
        Expanded(
          child: Text(
            text.toUpperCase(),
            style: const TextStyle(
              fontSize: 11,
              fontWeight: FontWeight.w600,
              letterSpacing: 0.9,
              color: NixxaColors.text3,
            ),
          ),
        ),
        ?trailing,
      ],
    ),
  );
}

class SliderRow extends StatelessWidget {
  final String label;
  final int value, min, max;
  final String Function(int)? format;
  final ValueChanged<int> onChanged;
  final ValueChanged<int>? onChangeEnd;
  const SliderRow({
    super.key,
    required this.label,
    required this.value,
    required this.min,
    required this.max,
    required this.onChanged,
    this.format,
    this.onChangeEnd,
  });

  @override
  Widget build(BuildContext context) {
    final v = value.clamp(min, max);
    return Row(
      children: [
        SizedBox(
          width: 108,
          child: Text(label, style: const TextStyle(color: NixxaColors.text2, fontSize: 13)),
        ),
        Expanded(
          child: Slider(
            value: v.toDouble(),
            min: min.toDouble(),
            max: max <= min ? min + 1.0 : max.toDouble(),
            divisions: max > min ? max - min : 1,
            label: (format ?? (x) => '$x')(v),
            onChanged: max <= min ? null : (d) => onChanged(d.round()),
            onChangeEnd: onChangeEnd == null ? null : (d) => onChangeEnd!(d.round()),
          ),
        ),
        SizedBox(
          width: 56,
          child: Text(
            (format ?? (x) => '$x')(v),
            textAlign: TextAlign.right,
            style: const TextStyle(fontFeatures: [FontFeature.tabularFigures()], fontSize: 13),
          ),
        ),
      ],
    );
  }
}

class SwitchRow extends StatelessWidget {
  final String label;
  final String? hint;
  final bool value;
  final ValueChanged<bool>? onChanged;
  const SwitchRow({super.key, required this.label, this.hint, required this.value, required this.onChanged});

  @override
  Widget build(BuildContext context) => SwitchListTile(
    contentPadding: EdgeInsets.zero,
    dense: true,
    title: Text(label, style: const TextStyle(fontSize: 14)),
    subtitle: hint == null ? null : Text(hint!, style: const TextStyle(fontSize: 12, color: NixxaColors.text3)),
    value: value,
    onChanged: onChanged,
  );
}

class Panel extends StatelessWidget {
  final Widget child;
  final EdgeInsetsGeometry padding;
  const Panel({super.key, required this.child, this.padding = const EdgeInsets.all(16)});

  @override
  Widget build(BuildContext context) => Container(
    padding: padding,
    decoration: BoxDecoration(
      color: NixxaColors.surface,
      borderRadius: BorderRadius.circular(14),
      border: Border.all(color: NixxaColors.line),
    ),
    child: child,
  );
}

void showMessage(BuildContext context, String text, {bool error = false}) {
  final m = ScaffoldMessenger.maybeOf(context);
  m?.hideCurrentSnackBar();
  m?.showSnackBar(
    SnackBar(
      content: Text(text, style: TextStyle(color: error ? NixxaColors.danger : NixxaColors.text)),
    ),
  );
}

/// Runs [action]; shows a readable snackbar on failure. Returns success.
Future<bool> guard(BuildContext context, Future<void> Function() action, {String? success}) async {
  try {
    await action();
    if (success != null && context.mounted) showMessage(context, success);
    return true;
  } on Object catch (e) {
    if (context.mounted) showMessage(context, errorMessage(e), error: true);
    return false;
  }
}

Future<bool> confirm(
  BuildContext context, {
  required String title,
  required String body,
  required String action,
  bool destructive = false,
}) async {
  final ok = await showDialog<bool>(
    context: context,
    builder: (ctx) => AlertDialog(
      title: Text(title),
      content: Text(body, style: const TextStyle(color: NixxaColors.text2)),
      actions: [
        TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
        TextButton(
          onPressed: () => Navigator.pop(ctx, true),
          style: TextButton.styleFrom(foregroundColor: destructive ? NixxaColors.danger : NixxaColors.accent),
          child: Text(action),
        ),
      ],
    ),
  );
  return ok ?? false;
}

class StatusDot extends StatelessWidget {
  final Color color;
  const StatusDot(this.color, {super.key});

  @override
  Widget build(BuildContext context) => Container(
    width: 8,
    height: 8,
    decoration: BoxDecoration(color: color, shape: BoxShape.circle),
  );
}
