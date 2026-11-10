import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/model/drawing.dart';
import 'package:nixxa/model/face.dart';
import 'package:nixxa/protocol/blob.dart';
import 'package:nixxa/ui/drawing_editor_screen.dart';
import 'package:nixxa/ui/face_editor_screen.dart';

import 'harness.dart';

Future<void> openEditor(WidgetTester tester, Widget screen) async {
  final nav = tester.state<NavigatorState>(find.byType(Navigator).first);
  nav.push(MaterialPageRoute<void>(builder: (_) => screen));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('editor: edit name and mood, Send to bunny = PUT_SLOT + ACTIVATE', (tester) async {
    final h = await pumpApp(tester, paired: true);
    await openEditor(tester, const FaceEditorScreen(face: FaceDef(name: 'BUNNY'), slot: 3));

    await tester.enterText(find.byKey(const Key('editor-name')), 'Hi there!');
    await tester.pump();
    await tester.tap(find.byKey(const Key('mood-love')));
    await tester.pump();

    await tester.tap(find.byKey(const Key('editor-send')));
    await tester.pumpAndSettle();
    // Slot 3 is suggested; pick it.
    await tester.tap(find.byKey(const Key('slot-pick-3')));
    await tester.pumpAndSettle();

    final blob = h.link.slotBlobs[3];
    expect(blob, isNotNull);
    final sent = decodeFace(Uint8List.sublistView(blob!, 0, 64));
    expect(sent.name, 'HI THERE'); // the field filters to the OLED font
    expect(sent.mood, Mood.love);
    expect(h.link.activeSlot, 3);
    expect(find.text('Sent to slot 4'), findsOneWidget);
  });

  testWidgets('editor: mood cycle multi-select keeps order and caps at 12', (tester) async {
    await pumpApp(tester, paired: true);
    await openEditor(tester, const FaceEditorScreen(face: FaceDef(name: 'CYCLE')));
    final list = find.descendant(of: find.byType(FaceEditorScreen), matching: find.byType(Scrollable)).first;
    await tester.scrollUntilVisible(find.text('Random'), 100, scrollable: list);
    await tester.tap(find.text('Random'));
    await tester.pumpAndSettle();
    final picks = Mood.values.take(13).toList();
    for (final m in picks) {
      final chip = find.byKey(Key('cycle-${m.name}'));
      await tester.scrollUntilVisible(chip, 100, scrollable: list);
      await tester.ensureVisible(chip);
      await tester.pumpAndSettle();
      await tester.tap(chip);
      await tester.pump();
    }
    await tester.scrollUntilVisible(find.text('12 of 12 moods, in this order'), -100, scrollable: list);
    expect(find.text('12 of 12 moods, in this order'), findsOneWidget);
    expect(find.text('1. Neutral'), findsOneWidget);
    final last = tester.widget<FilterChip>(find.byKey(Key('cycle-${picks.last.name}')));
    expect(last.selected, isFalse);
    expect(last.onSelected, isNull);
  });

  testWidgets('editor: save on phone adds it to the local library', (tester) async {
    final h = await pumpApp(tester, paired: true);
    await openEditor(
      tester,
      const FaceEditorScreen(
        face: FaceDef(name: 'KEEPER', mood: Mood.smug),
      ),
    );
    await tester.tap(find.byKey(const Key('editor-save')));
    await tester.pumpAndSettle();
    expect(h.store.faces.single.face.name, 'KEEPER');
    expect(h.app.saved.single.face.mood, Mood.smug);
  });

  testWidgets('drawing editor: draw, add a frame, Done returns the drawing', (tester) async {
    await pumpApp(tester, paired: true);
    Drawing? result;
    final nav = tester.state<NavigatorState>(find.byType(Navigator).first);
    nav.push(MaterialPageRoute<Drawing>(builder: (_) => const DrawingEditorScreen())).then((d) => result = d);
    await tester.pumpAndSettle();

    final canvas = find.byKey(const Key('drawing-canvas'));
    final box = tester.getRect(canvas);
    await tester.dragFrom(box.center, const Offset(60, 0));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('drawing-size')), findsOneWidget);

    await tester.tap(find.byKey(const Key('frame-add')));
    await tester.pumpAndSettle();
    expect(find.text('FRAMES 2 / 2'), findsOneWidget);

    await tester.tap(find.byKey(const Key('drawing-done')));
    await tester.pumpAndSettle();
    expect(result, isNotNull);
    expect(result!.frames, hasLength(2));
    expect(result!.frames[0].any((b) => b != 0), isTrue, reason: 'the stroke lit pixels in frame 1');
    expect(result!.frames[1].every((b) => b == 0), isTrue);
  });
}
