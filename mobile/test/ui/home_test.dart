// Home: the Faces entries, tapping the device card, and the pieces that moved
// off it (clock to Widgets, DND to Notifications, Forget only while connected).
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/app/face_store.dart';
import 'package:nixxa/app/text_raster.dart';
import 'package:nixxa/model/drawing.dart';
import 'package:nixxa/model/face.dart';
import 'package:nixxa/ui/drawing_editor_screen.dart';
import 'package:nixxa/ui/notifications_screen.dart';

import 'harness.dart';

void main() {
  testWidgets('Home lists Faces: Expressions, Widgets, Add GIF / images, Drawing, in that order', (tester) async {
    await pumpApp(tester, paired: true);
    expect(find.text('FACES'), findsOneWidget);
    expect(find.text('QUICK ACTIONS'), findsNothing);

    final ys = [
      for (final k in ['home-expressions', 'home-widgets', 'home-images', 'home-drawing'])
        tester.getTopLeft(find.byKey(Key(k))).dy,
    ];
    expect(ys, orderedEquals([...ys]..sort()));
    expect(ys.toSet(), hasLength(4));
    expect(find.text('Expressions'), findsOneWidget);
    expect(find.text('Widgets'), findsOneWidget);
    expect(find.text('Add GIF / images'), findsOneWidget);
    expect(find.text('Drawing'), findsOneWidget);
  });

  testWidgets('tapping the connected device opens the Device screen', (tester) async {
    await pumpApp(tester, paired: true);
    expect(find.byTooltip('Back'), findsNothing);
    await tester.tap(find.byKey(const Key('home-device')));
    await tester.pumpAndSettle();
    expect(find.byTooltip('Back'), findsOneWidget);
    expect(find.text('Faces stored'), findsOneWidget);
  });

  testWidgets('Expressions shows every preset and opens one in the editor', (tester) async {
    final store = MemoryFaceStore([const SavedFace(FaceDef(name: 'MINE', mood: Mood.love))]);
    await pumpApp(tester, paired: true, store: store);
    await tester.tap(find.byKey(const Key('home-expressions')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('expr-saved-MINE')), findsOneWidget);
    expect(find.byKey(const Key('expr-preset-0')), findsOneWidget);

    await tester.tap(find.byKey(const Key('expr-preset-0')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('editor-send')), findsOneWidget);
  });

  testWidgets('Widgets holds the clock picker; weather and air quality are marked as coming', (tester) async {
    final h = await pumpApp(tester, paired: true);
    await tester.tap(find.byKey(const Key('home-widgets')));
    await tester.pumpAndSettle();
    expect(find.text('Weather'), findsOneWidget);
    expect(find.text('Air quality'), findsOneWidget);
    expect(find.text('Shows the face'), findsOneWidget);

    await tester.tap(find.byKey(const Key('widgets-clock')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('clock-tile-2')));
    await tester.pumpAndSettle();
    expect(h.app.clockStyle, 2);
  });

  testWidgets('the Device screen no longer has a clock row', (tester) async {
    await pumpApp(tester, paired: true);
    await goTab(tester, 'Device');
    expect(find.byKey(const Key('device-clock')), findsNothing);
    expect(find.text('Clock'), findsNothing);
  });

  testWidgets('Forget is only possible while connected', (tester) async {
    final h = await pumpApp(tester, paired: true);
    await goTab(tester, 'Device');
    OutlinedButton forget() => tester.widget<OutlinedButton>(find.byKey(const Key('device-forget')));
    await tester.ensureVisible(find.byKey(const Key('device-forget')));
    expect(forget().onPressed, isNotNull);
    expect(find.byKey(const Key('device-forget-hint')), findsNothing);

    h.link.simulateDisconnect();
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.byKey(const Key('device-forget')));
    expect(forget().onPressed, isNull);
    expect(find.byKey(const Key('device-forget-hint')), findsOneWidget);

    h.link.simulateReconnect();
    await tester.pumpAndSettle();
    expect(forget().onPressed, isNotNull);
  });

  testWidgets('Forgetting from the Device screen opened on Home lands on Pair', (tester) async {
    await pumpApp(tester, paired: true);
    await tester.tap(find.byKey(const Key('home-device')));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.byKey(const Key('device-forget')));
    await tester.tap(find.byKey(const Key('device-forget')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Forget'));
    await tester.pumpAndSettle();
    expect(find.text('Pair your bunny'), findsOneWidget);
    expect(find.byTooltip('Back'), findsNothing);
  });

  testWidgets('Notifications has a Do Not Disturb switch that reaches the bunny', (tester) async {
    final h = await pumpApp(tester, paired: true);
    await goTab(tester, 'Notifications');
    final dnd = find.byKey(const Key('notif-dnd'));
    final list = find.descendant(of: find.byType(NotificationsScreen), matching: find.byType(Scrollable)).first;
    await tester.scrollUntilVisible(dnd, 100, scrollable: list);
    await tester.ensureVisible(dnd);
    await tester.pumpAndSettle();
    expect(h.link.dnd, isFalse);
    await tester.tap(dnd);
    await tester.pumpAndSettle();
    expect(h.link.dnd, isTrue);
    expect(h.app.device?.dnd, isTrue);
    await tester.tap(dnd);
    await tester.pumpAndSettle();
    expect(h.link.dnd, isFalse);

    h.link.simulateDisconnect();
    await tester.pumpAndSettle();
    final sw = tester.widget<SwitchListTile>(find.descendant(of: dnd, matching: find.byType(SwitchListTile)));
    expect(sw.onChanged, isNull);
  });

  testWidgets('Library previews what is stored in each slot', (tester) async {
    final h = await pumpApp(tester, paired: true);
    await seedSlot(h.link, 4, const FaceDef(name: 'GRUMPY', mood: Mood.angry));
    await h.app.refresh();
    await goTab(tester, 'Library');
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('slot-preview-4')), findsOneWidget);
    expect(find.byKey(const Key('slot-preview-0')), findsOneWidget);
    expect(find.byKey(const Key('slot-preview-5')), findsNothing, reason: 'empty slots have no preview');
    expect(h.app.slotFaces[4]?.face.name, 'GRUMPY');

    // Deleting the slot drops its preview.
    await h.app.deleteSlot(4);
    await tester.pumpAndSettle();
    expect(h.app.slotFaces.containsKey(4), isFalse);
    expect(find.byKey(const Key('slot-preview-4')), findsNothing);
  });

  testWidgets('text tool: type words, tap the canvas, pixels land in the frame', (tester) async {
    await pumpApp(tester, paired: true);
    Drawing? result;
    final nav = tester.state<NavigatorState>(find.byType(Navigator).first);
    nav.push(MaterialPageRoute<Drawing>(builder: (_) => const DrawingEditorScreen())).then((d) => result = d);
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('tool-text')));
    await tester.pumpAndSettle();
    final box = tester.getRect(find.byKey(const Key('drawing-canvas')));
    await tester.tapAt(box.topLeft + const Offset(12, 12));
    await tester.pumpAndSettle();
    expect(find.text('Add text'), findsOneWidget);
    await tester.enterText(find.byKey(const Key('text-input')), 'HI');
    await tester.tap(find.byKey(const Key('text-size-2')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('text-add')));
    // Rasterising the text needs the real event loop.
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 300)));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('drawing-done')));
    await tester.pumpAndSettle();
    expect(result, isNotNull);
    final lit = result!.frames[0].fold<int>(0, (n, b) => n + _bits(b));
    expect(lit, greaterThan(20), reason: 'the letters lit pixels');
  });

  testWidgets('text_raster: words give pixels inside a sane box, blanks give none', (tester) async {
    await tester.runAsync(() async {
      final pts = await textPixels('HELLO', size: 14);
      expect(pts, isNotEmpty);
      expect(pts.every((p) => p.$1 >= 0 && p.$2 >= 0 && p.$1 < 128 && p.$2 < 32), isTrue);
      final big = await textPixels('HELLO', size: 28);
      expect(big.length, greaterThan(pts.length));
      expect(await textPixels('   ', size: 14), isEmpty);
    });
  });
}

int _bits(int b) {
  var n = 0;
  for (var v = b & 0xFF; v != 0; v >>= 1) {
    n += v & 1;
  }
  return n;
}
