// Bottom sheet that picks one of the 16 device slots.
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../app/app_state.dart';
import '../../app/theme.dart';
import '../../protocol/bunny.dart';

Future<int?> pickSlot(BuildContext context, {int? suggested, String title = 'Send to which slot?'}) {
  return showModalBottomSheet<int>(
    context: context,
    isScrollControlled: true,
    builder: (ctx) => _SlotPicker(title: title, suggested: suggested),
  );
}

class _SlotPicker extends StatelessWidget {
  final String title;
  final int? suggested;
  const _SlotPicker({required this.title, this.suggested});

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(title, style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600)),
            const SizedBox(height: 4),
            const Text(
              'Sending replaces what is in the slot and shows it right away.',
              style: TextStyle(fontSize: 12, color: NixxaColors.text3),
            ),
            const SizedBox(height: 12),
            GridView.count(
              crossAxisCount: 4,
              shrinkWrap: true,
              physics: const NeverScrollableScrollPhysics(),
              mainAxisSpacing: 8,
              crossAxisSpacing: 8,
              childAspectRatio: 1.15,
              children: [
                for (var i = 0; i < Bunny.slots; i++)
                  _SlotTile(
                    slot: i,
                    name: app.isUsed(i) ? (app.slotName(i) ?? 'SLOT $i') : null,
                    active: app.activeSlot == i,
                    suggested: suggested == i,
                    onTap: () => Navigator.pop(context, i),
                  ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

class _SlotTile extends StatelessWidget {
  final int slot;
  final String? name;
  final bool active, suggested;
  final VoidCallback onTap;
  const _SlotTile({required this.slot, this.name, required this.active, required this.suggested, required this.onTap});

  @override
  Widget build(BuildContext context) => Material(
    color: suggested ? NixxaColors.accentSoft : NixxaColors.surface2,
    shape: RoundedRectangleBorder(
      borderRadius: BorderRadius.circular(8),
      side: BorderSide(color: suggested ? NixxaColors.accentLine : NixxaColors.line2),
    ),
    child: InkWell(
      key: Key('slot-pick-$slot'),
      borderRadius: BorderRadius.circular(8),
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.all(6),
        child: FittedBox(
          fit: BoxFit.scaleDown,
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Text('${slot + 1}', style: const TextStyle(fontWeight: FontWeight.w600)),
              const SizedBox(height: 2),
              Text(
                name ?? 'Empty',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(fontSize: 10, color: name == null ? NixxaColors.text3 : NixxaColors.text2),
              ),
              if (active) const Text('active', style: TextStyle(fontSize: 9, color: NixxaColors.ok)),
            ],
          ),
        ),
      ),
    ),
  );
}
