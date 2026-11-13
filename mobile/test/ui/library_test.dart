import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/app/face_store.dart';
import 'package:nixxa/model/face.dart';
import 'package:nixxa/ui/library_screen.dart';

import 'harness.dart';

void main() {
  testWidgets('library lists the 16 slots and tapping one activates it', (tester) async {
    final h = await pumpApp(tester, paired: true);
    await seedSlot(h.link, 4, const FaceDef(name: 'GRUMPY', mood: Mood.angry));
    await h.app.refresh();

    await goTab(tester, 'Library');
    for (var i = 0; i < 16; i++) {
      expect(find.byKey(Key('slot-$i')), findsOneWidget);
    }
    expect(find.text('GRUMPY'), findsOneWidget);

    await tester.tap(find.byKey(const Key('slot-4')));
    await tester.pumpAndSettle();
    expect(h.link.activeSlot, 4);
    expect(h.app.activeSlot, 4);
    expect(h.app.activeFace?.face.name, 'GRUMPY');
  });

  testWidgets('deleting a slot asks first and refuses nothing silently', (tester) async {
    final h = await pumpApp(tester, paired: true);
    await seedSlot(h.link, 2, const FaceDef(name: 'BYE'));
    await h.app.refresh();
    await goTab(tester, 'Library');

    await tester.longPress(find.byKey(const Key('slot-2')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Delete from bunny'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Delete'));
    await tester.pumpAndSettle();
    expect(h.link.slotBlobs.containsKey(2), isFalse);
    expect(find.text('BYE'), findsNothing);
  });

  testWidgets('a saved face can be sent to a chosen slot', (tester) async {
    final store = MemoryFaceStore([const SavedFace(FaceDef(name: 'SAVED', mood: Mood.love))]);
    final h = await pumpApp(tester, paired: true, store: store);
    await goTab(tester, 'Library');
    // The slot previews push "On this phone" below the fold.
    final list = find.descendant(of: find.byType(LibraryScreen), matching: find.byType(Scrollable)).first;
    await tester.scrollUntilVisible(find.byKey(const Key('saved-SAVED')), 100, scrollable: list);
    expect(find.byKey(const Key('saved-SAVED')), findsOneWidget);

    await tester.tap(find.byKey(const Key('saved-send-SAVED')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('slot-pick-7')));
    await tester.pumpAndSettle();
    expect(h.link.activeSlot, 7);
    expect(h.app.slotName(7), 'SAVED');
  });

  testWidgets('New face starts from a preset and opens the editor', (tester) async {
    await pumpApp(tester, paired: true);
    await goTab(tester, 'Library');
    await tester.tap(find.byTooltip('New face'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('preset-0')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('editor-send')), findsOneWidget);
  });

  test('face file round-trips through JSON with a drawing', () {
    final face = const FaceDef(
      name: 'ROUND TRIP',
      mood: Mood.starry,
      cycle: CycleMode.random,
      cycleMoods: [Mood.happy, Mood.sad],
    );
    final text = faceFileText(SavedFace(face));
    final back = parseFaceFile(text);
    expect(back.face, face);
    expect(back.drawing, isNull);
    expect(faceFileName(face), 'ROUND_TRIP.deskbunny.json');
    expect(() => parseFaceFile('not json'), throwsFormatException);
    expect(() => parseFaceFile('[1,2]'), throwsFormatException);
  });
}
