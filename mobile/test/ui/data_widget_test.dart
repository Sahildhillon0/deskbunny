// Weather and air-quality screens: choose a city, see the picture, send it.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/model/drawing.dart';
import 'package:nixxa/ui/widgets/oled.dart';

import 'harness.dart';

Future<void> openWidget(WidgetTester tester, String key) async {
  await tester.tap(find.byKey(const Key('home-widgets')));
  await tester.pumpAndSettle();
  await tester.tap(find.byKey(Key(key)));
  await tester.pumpAndSettle();
}

Future<void> pickCity(WidgetTester tester, {String query = 'pune', int index = 0}) async {
  await tester.tap(find.byKey(const Key('dw-place')));
  await tester.pumpAndSettle();
  await tester.enterText(find.byKey(const Key('place-query')), query);
  await tester.tap(find.byKey(const Key('place-search')));
  await tester.pumpAndSettle();
  await tester.tap(find.byKey(Key('place-$index')));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('Widgets lists Clock, Weather and Air quality, all open', (tester) async {
    await pumpApp(tester, paired: true);
    await tester.tap(find.byKey(const Key('home-widgets')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('widgets-clock')), findsOneWidget);
    expect(find.byKey(const Key('widgets-weather')), findsOneWidget);
    expect(find.byKey(const Key('widgets-air')), findsOneWidget);
    expect(find.text('Pick a city'), findsNWidgets(2));
    expect(find.textContaining('Coming soon'), findsNothing);
  });

  testWidgets('weather: choose a city, see the reading, flip the unit, send it to a slot', (tester) async {
    final h = await pumpApp(tester, paired: true);
    await openWidget(tester, 'widgets-weather');
    expect(find.text('Choose a city first'), findsOneWidget);
    expect(tester.widget<FilledButton>(find.byKey(const Key('dw-send'))).onPressed, isNull);

    await pickCity(tester);
    expect(h.data.searches, 1);
    expect(h.data.weatherCalls, 1);
    expect(h.app.place?.name, 'Pune');
    expect(find.text('Pune, Maharashtra, India'), findsOneWidget);
    expect(find.byType(OledFrame), findsOneWidget);

    OledFrame shown() => tester.widget<OledFrame>(find.byType(OledFrame));
    final celsius = List<int>.of(shown().fb);
    await tester.tap(find.text('°F'));
    await tester.pumpAndSettle();
    expect(h.app.fahrenheit, isTrue);
    expect(List<int>.of(shown().fb), isNot(celsius), reason: 'the number changed to Fahrenheit');
    expect(h.data.weatherCalls, 1, reason: 'switching the unit does not fetch again');

    await tester.tap(find.byKey(const Key('dw-refresh')));
    await tester.pumpAndSettle();
    expect(h.data.weatherCalls, 2);

    await tester.tap(find.byKey(const Key('dw-send')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('slot-pick-6')));
    await tester.pumpAndSettle();
    expect(h.link.activeSlot, 6);
    expect(h.app.slotName(6), 'WEATHER');
    final (_, drawing) = await h.app.loadSlot(6);
    expect(drawing, isNotNull);
    expect(drawing!.mode, DrawingMode.replace);
    expect(drawing.frames, hasLength(1));
    expect(drawing.frames.single, shown().fb, reason: 'the bunny got exactly the previewed picture');
  });

  testWidgets('air quality shares the city and sends under its own name', (tester) async {
    final h = await pumpApp(tester, paired: true);
    await openWidget(tester, 'widgets-weather');
    await pickCity(tester);
    await tester.pageBack();
    await tester.pumpAndSettle();
    expect(find.text('Pune'), findsNWidgets(2), reason: 'both rows show the chosen city');

    await tester.tap(find.byKey(const Key('widgets-air')));
    await tester.pumpAndSettle();
    expect(h.data.airCalls, 1, reason: 'a remembered city loads straight away');
    expect(find.byType(OledFrame), findsOneWidget);
    expect(find.byKey(const Key('dw-unit')), findsNothing, reason: 'no temperature unit here');

    await tester.tap(find.byKey(const Key('dw-send')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('slot-pick-9')));
    await tester.pumpAndSettle();
    expect(h.app.slotName(9), 'AIR QUALITY');
  });

  testWidgets('failures are readable and nothing is sent', (tester) async {
    final h = await pumpApp(tester, paired: true);
    h.data.failWith = 'No internet connection.';
    await openWidget(tester, 'widgets-weather');
    await tester.tap(find.byKey(const Key('dw-place')));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const Key('place-query')), 'pune');
    await tester.tap(find.byKey(const Key('place-search')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('place-error')), findsOneWidget);
    expect(find.text('No internet connection.'), findsOneWidget);

    // Back online: pick a city, then lose the connection on the next refresh.
    h.data.failWith = null;
    await tester.tap(find.byKey(const Key('place-search')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('place-0')));
    await tester.pumpAndSettle();
    h.data.failWith = 'The weather service did not answer in time. Check your connection.';
    await tester.tap(find.byKey(const Key('dw-refresh')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('dw-error')), findsOneWidget);
    expect(find.textContaining('did not answer in time'), findsOneWidget);
    expect(h.link.slotBlobs.containsKey(6), isFalse);
  });

  testWidgets('an unknown city says so', (tester) async {
    await pumpApp(tester, paired: true);
    await openWidget(tester, 'widgets-air');
    await tester.tap(find.byKey(const Key('dw-place')));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const Key('place-query')), 'nowhere');
    await tester.tap(find.byKey(const Key('place-search')));
    await tester.pumpAndSettle();
    expect(find.text('No city found. Check the spelling.'), findsOneWidget);
  });

  testWidgets('sending needs the bunny: the button says so while it is away', (tester) async {
    final h = await pumpApp(tester, paired: true);
    await openWidget(tester, 'widgets-weather');
    await pickCity(tester);
    h.link.simulateDisconnect();
    await tester.pumpAndSettle();
    expect(tester.widget<FilledButton>(find.byKey(const Key('dw-send'))).onPressed, isNull);
    expect(find.text('Connect to send'), findsOneWidget);
    expect(find.byType(OledFrame), findsOneWidget, reason: 'the preview still works offline from the bunny');
  });
}
