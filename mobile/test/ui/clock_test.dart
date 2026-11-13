import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/protocol/bunny.dart';
import 'package:nixxa/ui/clock_screen.dart';

import 'harness.dart';

void main() {
  testWidgets('clock picker renders Face plus 20 clocks, without overflow', (tester) async {
    final h = await pumpApp(tester, paired: true);
    await goTab(tester, 'Device');
    await tester.tap(find.byKey(const Key('device-clock')));
    await tester.pumpAndSettle();

    // The grid builds its tiles lazily, so count the delegate's children.
    final grid = find.descendant(of: find.byType(ClockScreen), matching: find.byType(GridView));
    final delegate = tester.widget<GridView>(grid).childrenDelegate as SliverChildListDelegate;
    expect(delegate.estimatedChildCount, 21);
    expect(find.byKey(const Key('clock-tile-0')), findsOneWidget);
    expect(find.text('Face'), findsOneWidget);
    await tester.dragUntilVisible(
      find.byKey(const Key('clock-tile-${Bunny.clockStyles}')),
      grid,
      const Offset(0, -200),
    );
    expect(find.byKey(const Key('clock-tile-${Bunny.clockStyles}')), findsOneWidget);
    expect(find.text('Clock ${Bunny.clockStyles}'), findsOneWidget);
    expect(tester.takeException(), isNull);
    expect(h.app.clockStyle, 0);
  });

  testWidgets('tapping a clock saves it on the phone and sends SET_CLOCK; Face switches back', (tester) async {
    final h = await pumpApp(tester, paired: true);
    await goTab(tester, 'Device');
    await tester.tap(find.byKey(const Key('device-clock')));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('clock-tile-5')));
    await tester.pumpAndSettle();
    expect(h.app.clockStyle, 5);
    expect(h.link.clockStyle, 5);
    expect(find.text('Clock'), findsWidgets); // the Device row's subtitle now names it
    expect(find.text('Clock 5'), findsWidgets);

    await tester.tap(find.byKey(const Key('clock-tile-0')));
    await tester.pumpAndSettle();
    expect(h.app.clockStyle, 0);
    expect(h.link.clockStyle, 0);
  });

  testWidgets('offline, the choice is still saved and the screen says so', (tester) async {
    final h = await pumpApp(tester, paired: true);
    h.link.simulateDisconnect();
    await tester.pumpAndSettle();
    await goTab(tester, 'Device');
    await tester.tap(find.byKey(const Key('device-clock')));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('clock-status')), findsOneWidget);
    expect(find.textContaining('not connected'), findsOneWidget);
    await tester.tap(find.byKey(const Key('clock-tile-3')));
    await tester.pumpAndSettle();
    expect(h.link.clockStyle, 3);
    expect(h.app.clockStyle, 3);
  });
}
