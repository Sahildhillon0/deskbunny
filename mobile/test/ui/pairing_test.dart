import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/link/device_link.dart';
import 'package:nixxa/link/fake_device_link.dart';

import 'harness.dart';

const validQr = 'NIXXA1:A1B2C3:00112233445566778899AABBCCDDEEFF';

void main() {
  testWidgets('unpaired shows Pair; a bad code shows a readable error', (tester) async {
    await pumpApp(tester);
    expect(find.text('Pair your bunny'), findsOneWidget);
    expect(find.byType(NavigationBar), findsNothing);

    await tester.enterText(find.byKey(const Key('pair-code')), 'hello');
    await tester.tap(find.byKey(const Key('pair-submit')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('pair-error')), findsOneWidget);
    expect(find.textContaining('not a nixxa QR code'), findsOneWidget);
  });

  testWidgets('a link failure is reported and the user can retry', (tester) async {
    final link = FakeDeviceLink(delay: Duration.zero)..failNextPair = 'NOT_FOUND';
    await pumpApp(tester, link: link);
    await tester.enterText(find.byKey(const Key('pair-code')), validQr);
    await tester.tap(find.byKey(const Key('pair-submit')));
    await tester.pumpAndSettle();
    expect(find.textContaining('No bunny found nearby'), findsOneWidget);

    await tester.tap(find.byKey(const Key('pair-submit')));
    await tester.pumpAndSettle();
    expect(find.byType(NavigationBar), findsOneWidget);
  });

  testWidgets('pairing with a valid code (lowercase ok) lands on Home, connected', (tester) async {
    final h = await pumpApp(tester);
    await tester.enterText(find.byKey(const Key('pair-code')), validQr.toLowerCase());
    await tester.tap(find.byKey(const Key('pair-submit')));
    await tester.pumpAndSettle();

    expect(h.app.status.state, LinkState.connected);
    expect(find.byType(NavigationBar), findsOneWidget);
    expect(find.byKey(const Key('home-status')), findsOneWidget);
    expect(find.text('Connected'), findsOneWidget);
    expect(find.textContaining('NIXXA-A1B2C3'), findsOneWidget);
    expect(find.textContaining('ESP32-C3'), findsOneWidget);
    // Refresh ran over the real Bunny codec: the default face in slot 0.
    expect(h.app.device?.activeSlot, 0);
    expect(find.textContaining('DESKBUNNY'), findsOneWidget);
  });

  testWidgets('Forget device asks first, then returns to Pair', (tester) async {
    await pumpApp(tester, paired: true);
    await goTab(tester, 'Device');
    await tester.tap(find.byKey(const Key('device-forget')));
    await tester.pumpAndSettle();
    expect(find.text('Forget this bunny?'), findsOneWidget);
    await tester.tap(find.text('Forget'));
    await tester.pumpAndSettle();
    expect(find.text('Pair your bunny'), findsOneWidget);
  });
}
