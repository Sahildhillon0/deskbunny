import 'package:flutter/material.dart';

import 'app/app_state.dart';
import 'app/link_factory.dart';
import 'ui/app.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  final state = AppState(link: createDeviceLink())..start();
  runApp(NixxaApp(state: state));
}
