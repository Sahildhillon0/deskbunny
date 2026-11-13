// The Widgets screen's auto-update controls and the battery, event, counter,
// status and now-playing screens.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/app/phone_data.dart';
import 'package:nixxa/app/widget_settings.dart';
import 'package:nixxa/ui/widgets/oled.dart';

import 'harness.dart';

Future<void> openWidget(WidgetTester tester, String key, {bool fromHome = true}) async {
  if (fromHome) {
    await tester.tap(find.byKey(const Key('home-widgets')));
    await tester.pumpAndSettle();
  }
  final row = find.byKey(Key(key));
  await tester.scrollUntilVisible(row, 200, scrollable: find.byType(Scrollable).first);
  await tester.pumpAndSettle();
  await tester.tap(row);
  await tester.pumpAndSettle();
}

/// Taps the switch inside a SwitchRow (its row can be taller than the tile).
Future<void> tapSwitch(WidgetTester tester, String key) async {
  final sw = find.descendant(of: find.byKey(Key(key)), matching: find.byType(Switch));
  await tester.ensureVisible(sw);
  await tester.pumpAndSettle();
  await tester.tap(sw);
  await tester.pumpAndSettle();
}

Future<void> sendTo(WidgetTester tester, int slot) async {
  await tester.ensureVisible(find.byKey(const Key('dw-send')));
  await tester.tap(find.byKey(const Key('dw-send')));
  await tester.pumpAndSettle();
  await tester.tap(find.byKey(Key('slot-pick-$slot')));
  await tester.pumpAndSettle();
  // Let the "Sent to slot" snackbar go: it sits over the controls below.
  await tester.pump(const Duration(seconds: 5));
  await tester.pumpAndSettle();
}

const song = NowPlaying(title: 'Blinding Lights', artist: 'The Weeknd', app: 'Spotify', durationMs: 200000, positionMs: 60000, playing: true);

void main() {
  group('auto-update', () {
    testWidgets('the switch reveals the intervals and saves what you choose', (tester) async {
      final h = await pumpApp(tester, paired: true);
      await tester.tap(find.byKey(const Key('home-widgets')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('auto-refresh')), findsNothing);

      await tester.tap(find.byKey(const Key('auto-on')));
      await tester.pumpAndSettle();
      expect(h.settings.settings.autoOn, isTrue);
      expect(find.byKey(const Key('auto-refresh')), findsOneWidget);
      expect(find.textContaining('Nothing is switched on yet'), findsOneWidget);

      await tester.tap(find.descendant(of: find.byKey(const Key('auto-refresh')), matching: find.text('1 h')));
      await tester.pumpAndSettle();
      await tester.tap(find.descendant(of: find.byKey(const Key('auto-rotate')), matching: find.text('5 min')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('auto-home')));
      await tester.pumpAndSettle();
      expect(h.settings.settings.refreshMins, 60);
      expect(h.settings.settings.rotateSecs, 300);
      expect(h.settings.settings.rotateHome, isFalse);

      await tester.tap(find.byKey(const Key('auto-on')));
      await tester.pumpAndSettle();
      expect(h.settings.settings.autoOn, isFalse);
      expect(find.byKey(const Key('auto-refresh')), findsNothing);
    });

    testWidgets('it counts the widgets in the rotation and shows what the background did', (tester) async {
      final h = await pumpApp(tester, paired: true);
      h.app.updateSettings(
        (s) => s.copyWith(autoOn: true, slots: {'battery': 12, 'counter': 13}, rotation: {'battery', 'counter'}),
      );
      h.settings.status = BgStatus(
        lastRefresh: DateTime(2026, 11, 13, 10, 42),
        showingSlot: 12,
        error: 'WEATHER: No internet connection.',
      );
      await tester.tap(find.byKey(const Key('home-widgets')));
      await tester.pumpAndSettle();
      await h.app.refreshBgStatus();
      await tester.pumpAndSettle();
      expect(find.text('2 widgets in the rotation · last refreshed 10:42 · showing slot 13'), findsOneWidget);
      expect(find.text('WEATHER: No internet connection.'), findsOneWidget);
    });

    testWidgets('every widget has a row', (tester) async {
      await pumpApp(tester, paired: true);
      await tester.tap(find.byKey(const Key('home-widgets')));
      await tester.pumpAndSettle();
      for (final k in [
        'widgets-clock',
        'widgets-nowplaying',
        'widgets-weather',
        'widgets-air',
        'widgets-battery',
        'widgets-event',
        'widgets-counter',
        'widgets-status',
      ]) {
        await tester.scrollUntilVisible(find.byKey(Key(k)), 200, scrollable: find.byType(Scrollable).first);
        expect(find.byKey(Key(k)), findsOneWidget, reason: k);
      }
    });
  });

  group('phone battery', () {
    testWidgets('shows the charge, sends it, and joins the rotation only once it has a slot', (tester) async {
      final h = await pumpApp(tester, paired: true);
      h.phone.batteryNow = const PhoneBattery(87, true);
      await openWidget(tester, 'widgets-battery');
      expect(find.byType(OledFrame), findsOneWidget);
      final rot = find.byKey(const Key('dw-rotation'));
      await tester.ensureVisible(rot);
      expect(tester.widget<SwitchListTile>(find.descendant(of: rot, matching: find.byType(SwitchListTile))).onChanged, isNull);
      expect(find.text('Send it once first, to choose its slot'), findsOneWidget);

      await sendTo(tester, 12);
      expect(h.app.slotName(12), 'BATTERY');
      expect(h.settings.settings.slots['battery'], 12);

      await tapSwitch(tester, 'dw-rotation');
      expect(h.settings.settings.rotation, {'battery'});
      await tapSwitch(tester, 'dw-rotation');
      expect(h.settings.settings.rotation, isEmpty);
    });

    testWidgets('a slot belongs to one widget: taking it asks first', (tester) async {
      final h = await pumpApp(tester, paired: true);
      await openWidget(tester, 'widgets-battery');
      await sendTo(tester, 12);
      await tapSwitch(tester, 'dw-rotation');
      await tester.pageBack();
      await tester.pumpAndSettle();

      await openWidget(tester, 'widgets-counter', fromHome: false);
      await sendTo(tester, 12);
      expect(find.text('Replace Phone battery?'), findsOneWidget);
      await tester.tap(find.text('Replace'));
      await tester.pumpAndSettle();
      expect(h.settings.settings.slots, {'counter': 12});
      expect(h.settings.settings.rotation, isEmpty, reason: 'the battery widget lost its slot and its place');
      expect(h.app.slotName(12), 'COUNTER');
    });
  });

  group('next event', () {
    testWidgets('asks for calendar access, then shows the event picture', (tester) async {
      final h = await pumpApp(tester, paired: true);
      h.phone
        ..calendar = false
        ..event = CalendarEvent('Design review', DateTime.now().add(const Duration(hours: 3)));
      await openWidget(tester, 'widgets-event');
      expect(find.byKey(const Key('ev-allow')), findsOneWidget);
      expect(find.byType(OledFrame), findsNothing);
      expect(find.text('Calendar access is off'), findsOneWidget);

      await tester.tap(find.byKey(const Key('ev-allow')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('ev-allow')), findsNothing);
      expect(find.byType(OledFrame), findsOneWidget);

      await sendTo(tester, 11);
      expect(h.app.slotName(11), 'NEXT EVENT');
    });
  });

  group('counter', () {
    testWidgets('plus, minus, reset and the label are saved, and it sends as COUNTER', (tester) async {
      final h = await pumpApp(tester, paired: true);
      await openWidget(tester, 'widgets-counter');
      for (var i = 0; i < 3; i++) {
        await tester.tap(find.byKey(const Key('ct-plus')));
      }
      await tester.tap(find.byKey(const Key('ct-minus')));
      await tester.pumpAndSettle();
      expect(h.settings.settings.counter, 2);
      expect(find.byKey(const Key('ct-value')), findsOneWidget);
      expect(tester.widget<Text>(find.byKey(const Key('ct-value'))).data, '2');

      await tester.enterText(find.byKey(const Key('ct-label')), 'glasses of water');
      await tester.pumpAndSettle();
      expect(h.settings.settings.counterLabel, 'GLASSES OF WATER');

      await sendTo(tester, 10);
      expect(h.app.slotName(10), 'COUNTER');
      await tester.tap(find.byKey(const Key('ct-reset')));
      await tester.pumpAndSettle();
      expect(h.settings.settings.counter, 0);
    });
  });

  group('status sign', () {
    testWidgets('presets, your own words, and Do Not Disturb with the sign', (tester) async {
      final h = await pumpApp(tester, paired: true);
      await openWidget(tester, 'widgets-status');
      await tester.tap(find.byKey(const Key('st-preset-1')));
      await tester.pumpAndSettle();
      expect(h.settings.settings.statusText, 'FOCUS');

      await tester.enterText(find.byKey(const Key('st-text')), 'gone fishing');
      await tester.pumpAndSettle();
      expect(h.settings.settings.statusText, 'GONE FISHING');

      expect(find.byKey(const Key('dw-rotation')), findsNothing, reason: 'a sign is not part of the rotation');
      await tester.tap(find.byKey(const Key('st-dnd')));
      await tester.pumpAndSettle();
      expect(h.link.dnd, isFalse);
      await sendTo(tester, 8);
      expect(h.app.slotName(8), 'STATUS');
      expect(h.link.dnd, isTrue, reason: 'Do Not Disturb went on with the sign');
    });

    testWidgets('without the switch, sending a sign leaves Do Not Disturb alone', (tester) async {
      final h = await pumpApp(tester, paired: true);
      await openWidget(tester, 'widgets-status');
      await sendTo(tester, 8);
      expect(h.link.dnd, isFalse);
    });
  });

  group('now playing', () {
    testWidgets('without notification access it explains and offers the switch', (tester) async {
      final h = await pumpApp(tester, paired: true);
      h.link.notificationAccess = false;
      await openWidget(tester, 'widgets-nowplaying');
      expect(find.byKey(const Key('np-access')), findsOneWidget);
      expect(find.text('Notification access is off'), findsOneWidget);
      final follow = find.descendant(of: find.byKey(const Key('np-follow')), matching: find.byType(SwitchListTile));
      expect(tester.widget<SwitchListTile>(follow).onChanged, isNull);
    });

    testWidgets('shows the song live, and "show the song" asks for a slot, then saves it', (tester) async {
      final h = await pumpApp(tester, paired: true);
      h.link.notificationAccess = true;
      h.phone.song = song;
      await openWidget(tester, 'widgets-nowplaying');
      expect(find.byKey(const Key('np-access')), findsNothing);
      expect(find.byType(OledFrame), findsOneWidget);

      await tester.tap(find.byKey(const Key('np-follow')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('slot-pick-9')));
      await tester.pumpAndSettle();
      expect(h.settings.settings.musicOn, isTrue);
      expect(h.settings.settings.slots['nowplaying'], 9);

      await tester.tap(find.byKey(const Key('np-follow')));
      await tester.pumpAndSettle();
      expect(h.settings.settings.musicOn, isFalse);
      expect(h.settings.settings.slots['nowplaying'], 9, reason: 'the slot is remembered');
    });

    testWidgets('a song change reaches the preview without leaving the screen', (tester) async {
      final h = await pumpApp(tester, paired: true);
      h.link.notificationAccess = true;
      h.phone.song = song;
      await openWidget(tester, 'widgets-nowplaying');
      final before = List<int>.of(tester.widget<OledFrame>(find.byType(OledFrame)).fb);
      h.phone.play(const NowPlaying(title: 'Levitating', artist: 'Dua Lipa', app: 'YT Music', durationMs: 203000, positionMs: 1000, playing: true));
      await tester.pumpAndSettle();
      expect(List<int>.of(tester.widget<OledFrame>(find.byType(OledFrame)).fb), isNot(before));
    });

    testWidgets('nothing playing says so, and sending is not offered', (tester) async {
      final h = await pumpApp(tester, paired: true);
      h.link.notificationAccess = true;
      await openWidget(tester, 'widgets-nowplaying');
      expect(find.text('Nothing is playing'), findsOneWidget);
      expect(tester.widget<FilledButton>(find.byKey(const Key('dw-send'))).onPressed, isNull);
    });
  });
}
