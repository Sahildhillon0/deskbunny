// 7. Device: firmware and chip, battery-optimisation exemption, Forget device.
// On iOS there is no battery exemption; the screen explains the Bluetooth
// settings instead (ARCHITECTURE.md "iOS (phase 3)").
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../app/labels.dart';
import '../app/theme.dart';
import 'home_screen.dart' show linkColor;
import 'widgets/common.dart';

class DeviceScreen extends StatelessWidget {
  const DeviceScreen({super.key});

  Future<void> _forget(BuildContext context) async {
    final app = context.read<AppState>();
    final ok = await confirm(
      context,
      title: 'Forget this bunny?',
      body:
          'The bunny goes back to showing its QR code and this phone stops connecting to it. '
          'The faces stored on it are kept. You can pair again by scanning the code.',
      action: 'Forget',
      destructive: true,
    );
    if (!ok || !context.mounted) return;
    final nav = Navigator.of(context);
    if (await guard(context, app.forget)) {
      // Opened from Home, this screen sits above the root: leave it, so the
      // Pair screen is what the user sees.
      nav.popUntil((route) => route.isFirst);
    }
  }

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    final s = app.status;
    final ios = defaultTargetPlatform == TargetPlatform.iOS;
    final name = s.deviceId == null ? 'your bunny' : 'NIXXA-${s.deviceId}';
    final d = app.device;
    Widget info(String k, String v) => Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Row(
        children: [
          SizedBox(
            width: 120,
            child: Text(k, style: const TextStyle(color: NixxaColors.text2, fontSize: 13)),
          ),
          Expanded(child: Text(v, style: const TextStyle(fontSize: 13))),
        ],
      ),
    );
    return Scaffold(
      appBar: AppBar(title: const Text('Device')),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
        children: [
          Panel(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    StatusDot(linkColor(s.state)),
                    const SizedBox(width: 8),
                    Flexible(
                      child: Text(linkStateLabel(s.state), style: const TextStyle(fontWeight: FontWeight.w600)),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                info('Name', s.deviceId == null ? '—' : 'NIXXA-${s.deviceId}'),
                info('Chip', s.chip == null ? '—' : chipLabel(s.chip)),
                info('Firmware', s.fwVersion ?? '—'),
                info('Faces stored', d == null ? '—' : '${_bits(d.usedMask)} of 16'),
                info('Showing slot', d == null ? '—' : '${d.activeSlot + 1}'),
                info('Do Not Disturb', d == null ? '—' : (d.dnd ? 'On' : 'Off')),
              ],
            ),
          ),
          const SectionLabel('Stay connected'),
          if (ios)
            Text(
              'iOS keeps the bunny connected in the background, and the bunny reads notifications straight '
              'from iOS, even when nixxa is closed. For that, in Settings > Bluetooth tap (i) next to $name '
              'and turn on Share System Notifications.',
              key: const Key('device-ios-hint'),
              style: const TextStyle(fontSize: 13, color: NixxaColors.text2, height: 1.4),
            )
          else ...[
            const Text(
              'Android may stop nixxa in the background to save battery, and then notifications stop reaching the bunny. '
              'Let nixxa run unrestricted to keep the link up.',
              style: TextStyle(fontSize: 13, color: NixxaColors.text2, height: 1.4),
            ),
            const SizedBox(height: 10),
            OutlinedButton.icon(
              key: const Key('device-battery'),
              onPressed: () => guard(context, app.link.requestBatteryExemption),
              icon: const Icon(Icons.battery_saver_outlined),
              label: const Text('Allow running in the background'),
            ),
          ],
          const SectionLabel('Pairing'),
          OutlinedButton.icon(
            key: const Key('device-forget'),
            style: OutlinedButton.styleFrom(
              foregroundColor: NixxaColors.danger,
              side: const BorderSide(color: Color(0x59F2938A)),
            ),
            // The bunny has to hear the UNPAIR, or it stays locked to this phone.
            onPressed: app.connected ? () => _forget(context) : null,
            icon: const Icon(Icons.link_off),
            label: const Text('Forget device'),
          ),
          const SizedBox(height: 8),
          if (!app.connected)
            const Text(
              'Connect to the bunny to forget it. It has to be told to let go of this phone.',
              key: Key('device-forget-hint'),
              style: TextStyle(fontSize: 12, color: NixxaColors.text3),
            ),
          if (!app.connected) const SizedBox(height: 4),
          if (ios)
            Text(
              'iOS keeps its own Bluetooth pairing: after forgetting, also choose Forget This Device for $name '
              'in Settings > Bluetooth before you pair again.',
              style: const TextStyle(fontSize: 12, color: NixxaColors.text3),
            ),
          if (ios) const SizedBox(height: 4),
          const Text(
            'Lost the phone? Flashing the bunny from the desktop app also resets its pairing.',
            style: TextStyle(fontSize: 12, color: NixxaColors.text3),
          ),
        ],
      ),
    );
  }

  static int _bits(int m) {
    var n = 0;
    for (var i = 0; i < 16; i++) {
      if (m & (1 << i) != 0) n++;
    }
    return n;
  }
}
