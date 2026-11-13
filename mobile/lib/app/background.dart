// The widget scheduler's own Dart entry point. The Android link service starts
// a headless Flutter engine on `backgroundMain` (lib/main.dart), so widgets
// keep updating and rotating while the app is closed. Settings come from the
// same storage the app writes; the link and phone data come over the same
// native channels the app uses.
import 'package:flutter/widgets.dart';

import '../link/device_link_channel.dart';
import '../protocol/bunny.dart';
import 'phone_data.dart';
import 'widget_data.dart';
import 'widget_scheduler.dart';
import 'widget_settings.dart';

WidgetScheduler? _scheduler;

void runWidgetBackground() {
  WidgetsFlutterBinding.ensureInitialized();
  if (_scheduler != null) return;
  final link = DeviceLinkChannel();
  _scheduler = WidgetScheduler(
    link: link,
    bunny: Bunny(link),
    data: OpenMeteo(),
    phone: ChannelPhoneApi(),
    store: PrefsSettingsStore(),
  )..start();
}
