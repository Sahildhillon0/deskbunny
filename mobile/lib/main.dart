import 'package:flutter/material.dart';

import 'app/app_state.dart';
import 'app/background.dart';
import 'app/link_factory.dart';
import 'ui/app.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  final state = AppState(link: createDeviceLink())..start();
  runApp(NixxaApp(state: state));
}

/// Entry point of the headless engine the Android link service hosts: the
/// widget scheduler (see lib/app/background.dart).
@pragma('vm:entry-point')
void backgroundMain() => runWidgetBackground();
