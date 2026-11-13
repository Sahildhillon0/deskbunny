// iOS-only branches of the shared screens (ARCHITECTURE.md "iOS (phase 3)"):
// the Bluetooth-settings hints, the bundle-ID entry and no battery button.
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/ui/device_screen.dart';
import 'package:nixxa/ui/notifications_screen.dart';

import 'harness.dart';

Future<void> _scrollTo(WidgetTester tester, Finder target) async {
  final list = find.descendant(of: find.byType(NotificationsScreen), matching: find.byType(Scrollable)).first;
  await tester.scrollUntilVisible(target, 100, scrollable: list);
  await tester.ensureVisible(target);
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('iOS: Share System Notifications hint, bundle ID entry, no battery button', (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.iOS;
    try {
      final h = await pumpApp(tester, paired: true);
      await goTab(tester, 'Notifications');

      // Access card talks about the Bluetooth setting, not Android's listener access.
      expect(find.text('iOS isn\'t sharing notifications with the bunny'), findsOneWidget);
      expect(find.textContaining('Share System Notifications'), findsOneWidget);
      expect(find.textContaining('NIXXA-A1B2C3'), findsOneWidget);
      await tester.tap(find.byKey(const Key('notif-access')));
      await tester.pumpAndSettle();
      expect(find.text('iOS shares notifications with the bunny'), findsOneWidget);

      // Bundle ID entry (inside the Apps dropdown): invalid input is refused,
      // a valid one adds a shown rule.
      await _scrollTo(tester, find.byKey(const Key('notif-apps')));
      await tester.tap(find.byKey(const Key('notif-apps')));
      await tester.pumpAndSettle();
      final field = find.byKey(const Key('notif-bundle-id'));
      await _scrollTo(tester, field);
      await tester.enterText(field, 'not a bundle id');
      await tester.tap(find.byKey(const Key('notif-bundle-add')));
      await tester.pumpAndSettle();
      expect((await h.link.getNotifConfig()).rules, isEmpty);

      await tester.enterText(field, ' com.example.chat ');
      await tester.tap(find.byKey(const Key('notif-bundle-add')));
      await tester.pumpAndSettle();
      final rules = (await h.link.getNotifConfig()).rules;
      expect(rules.single.appId, 'com.example.chat');
      expect(rules.single.show, isTrue);
      expect(find.byKey(const Key('rule-com.example.chat')), findsOneWidget);

      // Device screen: Bluetooth hint instead of the battery exemption.
      await goTab(tester, 'Device');
      expect(find.byKey(const Key('device-ios-hint')), findsOneWidget);
      expect(find.byKey(const Key('device-battery')), findsNothing);
      // Scroll to the iOS note at the bottom of the Device list.
      await tester.dragUntilVisible(
        find.textContaining('Forget This Device'),
        find.descendant(of: find.byType(DeviceScreen), matching: find.byType(Scrollable)),
        const Offset(0, -200),
      );
      expect(find.textContaining('Forget This Device'), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('Android (default): no iOS-only widgets', (tester) async {
    await pumpApp(tester, paired: true);
    await goTab(tester, 'Notifications');
    expect(find.byKey(const Key('notif-bundle-id')), findsNothing);
    expect(find.text('Allow notification access'), findsOneWidget);
    await goTab(tester, 'Device');
    expect(find.byKey(const Key('device-battery')), findsOneWidget);
    expect(find.byKey(const Key('device-ios-hint')), findsNothing);
  });
}
