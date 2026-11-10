// Root widget: Pair screen while unpaired, bottom navigation once paired.
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../app/theme.dart';
import 'device_screen.dart';
import 'home_screen.dart';
import 'library_screen.dart';
import 'notifications_screen.dart';
import 'pair_screen.dart';

class NixxaApp extends StatelessWidget {
  final AppState state;
  const NixxaApp({super.key, required this.state});

  @override
  Widget build(BuildContext context) => ChangeNotifierProvider<AppState>.value(
    value: state,
    child: MaterialApp(
      title: 'nixxa',
      debugShowCheckedModeBanner: false,
      theme: buildNixxaTheme(),
      darkTheme: buildNixxaTheme(),
      themeMode: ThemeMode.dark,
      home: const _Root(),
    ),
  );
}

class _Root extends StatelessWidget {
  const _Root();

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    if (!app.statusKnown) {
      return const Scaffold(body: Center(child: CircularProgressIndicator()));
    }
    return AnimatedSwitcher(
      duration: const Duration(milliseconds: 200),
      child: app.paired ? const Shell(key: ValueKey('shell')) : const PairScreen(key: ValueKey('pair')),
    );
  }
}

class Shell extends StatefulWidget {
  const Shell({super.key});

  @override
  State<Shell> createState() => _ShellState();
}

class _ShellState extends State<Shell> {
  int _tab = 0;

  void _go(int i) => setState(() => _tab = i);

  @override
  Widget build(BuildContext context) {
    final pages = [
      HomeScreen(onOpenLibrary: () => _go(1)),
      const LibraryScreen(),
      const NotificationsScreen(),
      const DeviceScreen(),
    ];
    return Scaffold(
      // Tickers (the live preview) pause on hidden tabs to save battery.
      body: IndexedStack(
        index: _tab,
        children: [for (var i = 0; i < pages.length; i++) TickerMode(enabled: i == _tab, child: pages[i])],
      ),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _tab,
        onDestinationSelected: _go,
        destinations: const [
          NavigationDestination(icon: Icon(Icons.face_outlined), selectedIcon: Icon(Icons.face), label: 'Home'),
          NavigationDestination(
            icon: Icon(Icons.grid_view_outlined),
            selectedIcon: Icon(Icons.grid_view),
            label: 'Library',
          ),
          NavigationDestination(
            icon: Icon(Icons.notifications_none),
            selectedIcon: Icon(Icons.notifications),
            label: 'Notifications',
          ),
          NavigationDestination(icon: Icon(Icons.memory_outlined), selectedIcon: Icon(Icons.memory), label: 'Device'),
        ],
      ),
    );
  }
}
