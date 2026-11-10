// Smoke test: the app boots with the fake link and shows the Pair screen.
import 'package:flutter_test/flutter_test.dart';

import 'ui/harness.dart';

void main() {
  testWidgets('app boots to the Pair screen when unpaired', (tester) async {
    await pumpApp(tester);
    expect(find.text('Pair your bunny'), findsOneWidget);
    expect(find.text('Scan QR code'), findsOneWidget);
  });

  testWidgets('app boots to Home with bottom navigation when paired', (tester) async {
    await pumpApp(tester, paired: true);
    expect(find.text('Connected'), findsOneWidget);
    for (final tab in ['Home', 'Library', 'Notifications', 'Device']) {
      expect(find.text(tab), findsWidgets);
    }
  });
}
