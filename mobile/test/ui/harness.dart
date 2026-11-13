// Shared setup for the UI widget tests: the in-memory FakeDeviceLink, the real
// Bunny codec on top of it, a fake preview (no native engine) and an
// in-memory face store.
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/app/app_state.dart';
import 'package:nixxa/app/face_store.dart';
import 'package:nixxa/app/preview.dart';
import 'package:nixxa/link/fake_device_link.dart';
import 'package:nixxa/model/drawing.dart';
import 'package:nixxa/model/face.dart';
import 'package:nixxa/protocol/bunny.dart';
import 'package:nixxa/ui/app.dart';
import 'package:nixxa/ui/widgets/oled.dart';

/// Lights one pixel per frame so painting is exercised; records calls.
class FakePreview implements PreviewSource {
  static int created = 0;
  FaceDef face;
  Drawing? drawing;
  FakePreview(this.face, this.drawing) {
    created++;
  }

  @override
  void setFace(FaceDef face) => this.face = face;
  @override
  void setDrawing(Drawing? drawing) => this.drawing = drawing;
  @override
  Uint8List frame(int tMs) => Uint8List(1024)..[(tMs ~/ 33) % 1024] = 1;
  @override
  void dispose() {}
}

PreviewSource fakePreview(FaceDef face, Drawing? drawing) => FakePreview(face, drawing);

/// Clock stand-in (no native engine): one lit pixel per style, named "Clock N".
final ClockRenderer fakeClocks = ClockRenderer(
  frame: (style, h, m, s) => Uint8List(1024)..[style] = 1,
  name: (style) => 'Clock $style',
);

class Harness {
  final FakeDeviceLink link;
  final AppState app;
  final MemoryFaceStore store;
  Harness(this.link, this.app, this.store);
}

/// Pumps the whole app on a phone-sized screen.
Future<Harness> pumpApp(
  WidgetTester tester, {
  bool paired = false,
  FakeDeviceLink? link,
  MemoryFaceStore? store,
}) async {
  OledPreview.animationsEnabled = false;
  tester.view.physicalSize = const Size(1080, 2340);
  tester.view.devicePixelRatio = 2.75;
  addTearDown(tester.view.reset);
  final l = link ?? FakeDeviceLink(delay: Duration.zero, paired: paired);
  final s = store ?? MemoryFaceStore();
  final app = AppState(
    link: l,
    bunny: Bunny(l),
    store: s,
    previewFactory: fakePreview,
    clockRenderer: fakeClocks,
  );
  await app.start();
  await tester.pumpWidget(NixxaApp(state: app));
  await tester.pumpAndSettle();
  return Harness(l, app, s);
}

/// Puts [face] into [slot] on the fake device, as the phone would.
Future<void> seedSlot(FakeDeviceLink link, int slot, FaceDef face, [Drawing? drawing]) async {
  await link.request(BunnyOp.putSlot, Bunny.encodePutSlot(slot, face, drawing));
}

/// Home > Widgets > Clock.
Future<void> openClock(WidgetTester tester) async {
  await tester.tap(find.byKey(const Key('home-widgets')));
  await tester.pumpAndSettle();
  await tester.tap(find.byKey(const Key('widgets-clock')));
  await tester.pumpAndSettle();
}

Future<void> goTab(WidgetTester tester, String label) async {
  await tester.tap(find.descendant(of: find.byType(NavigationBar), matching: find.text(label)));
  await tester.pumpAndSettle();
}
