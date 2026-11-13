import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/model/face.dart';
import 'package:nixxa/ui/notifications_screen.dart';

import 'harness.dart';

void main() {
  testWidgets('notification settings round-trip through the link', (tester) async {
    final h = await pumpApp(tester, paired: true);
    await goTab(tester, 'Notifications');
    final list = find.descendant(of: find.byType(NotificationsScreen), matching: find.byType(Scrollable)).first;

    // Access: not granted -> button -> granted.
    expect(find.byKey(const Key('notif-access')), findsOneWidget);
    await tester.tap(find.byKey(const Key('notif-access')));
    await tester.pumpAndSettle();
    expect(find.text('Notification access allowed'), findsOneWidget);

    // Master switch off.
    await tester.tap(find.byKey(const Key('notif-enabled')));
    await tester.pumpAndSettle();
    expect((await h.link.getNotifConfig()).enabled, isFalse);
    expect(h.link.notifEnabled, isFalse);

    // Quiet hours on: 22:00 .. 07:00.
    await tester.tap(find.byKey(const Key('notif-quiet')));
    await tester.pumpAndSettle();
    var c = await h.link.getNotifConfig();
    expect((c.quietStartMin, c.quietEndMin), (22 * 60, 7 * 60));

    // Turn on Discord, found through search, and give it a mood.
    await tester.enterText(find.byKey(const Key('notif-search')), 'disc');
    await tester.pumpAndSettle();
    expect(find.text('Gmail'), findsNothing);
    final toggle = find.byKey(const Key('rule-com.discord'));
    await tester.scrollUntilVisible(toggle, 100, scrollable: list);
    await tester.ensureVisible(toggle);
    await tester.pumpAndSettle();
    await tester.tap(toggle);
    await tester.pumpAndSettle();
    c = await h.link.getNotifConfig();
    expect(c.rules.single.appId, 'com.discord');
    expect(c.rules.single.show, isTrue);
    expect(c.rules.single.mood, 0xFF);

    await tester.tap(find.byKey(const Key('rule-mood-com.discord')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Excited').last);
    await tester.pumpAndSettle();
    c = await h.link.getNotifConfig();
    expect(c.rules.single.mood, Mood.excited.index);

    // Off again removes the rule (mood was set, so it is kept but hidden).
    await tester.tap(toggle);
    await tester.pumpAndSettle();
    c = await h.link.getNotifConfig();
    expect(c.rules.single.show, isFalse);

    // Test button reaches the link.
    final test = find.byKey(const Key('notif-test'));
    await tester.scrollUntilVisible(test, 100, scrollable: list);
    await tester.ensureVisible(test);
    await tester.pumpAndSettle();
    await tester.tap(test);
    await tester.pumpAndSettle();
    expect(h.link.testNotifications, 1);
  });
}
