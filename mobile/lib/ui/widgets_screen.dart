// Widgets: screens the bunny can show instead of a face. Today that is the
// clock; weather and air quality need the phone to fetch data from the
// internet, which nixxa does not do yet.
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../app/theme.dart';
import 'clock_screen.dart';
import 'widgets/common.dart';

class WidgetsScreen extends StatelessWidget {
  const WidgetsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    return Scaffold(
      appBar: AppBar(title: const Text('Widgets')),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
        children: [
          const SectionLabel('Available'),
          Card(
            child: ListTile(
              key: const Key('widgets-clock'),
              leading: const Icon(Icons.schedule),
              title: const Text('Clock'),
              subtitle: Text(
                app.clockStyle == 0 ? 'Shows the face' : app.clockRenderer.name(app.clockStyle),
                style: const TextStyle(fontSize: 12, color: NixxaColors.text3),
              ),
              trailing: const Icon(Icons.chevron_right, color: NixxaColors.text3),
              onTap: () => Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => const ClockScreen())),
            ),
          ),
          const SectionLabel('Coming soon'),
          const _Soon(icon: Icons.wb_sunny_outlined, title: 'Weather'),
          const _Soon(icon: Icons.air, title: 'Air quality'),
        ],
      ),
    );
  }
}

class _Soon extends StatelessWidget {
  final IconData icon;
  final String title;
  const _Soon({required this.icon, required this.title});

  @override
  Widget build(BuildContext context) => Card(
    child: ListTile(
      enabled: false,
      leading: Icon(icon),
      title: Text(title),
      subtitle: const Text('Needs internet access', style: TextStyle(fontSize: 12)),
    ),
  );
}
