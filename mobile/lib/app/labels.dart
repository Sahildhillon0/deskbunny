// Human labels for enums and errors.
import '../link/device_link.dart';
import '../model/face.dart';
import 'widget_data.dart';

String moodLabel(Mood m) => switch (m) {
  Mood.eyeroll => 'Eye roll',
  _ => m.name[0].toUpperCase() + m.name.substring(1),
};

String moodIndexLabel(int index) =>
    index >= 0 && index < Mood.values.length ? moodLabel(Mood.values[index]) : 'Default';

String overlayLabel(FaceOverlay o) => switch (o) {
  FaceOverlay.none => 'None',
  FaceOverlay.hearts => 'Hearts',
  FaceOverlay.zzz => 'Zzz',
  FaceOverlay.sweat => 'Sweat',
  FaceOverlay.anger => 'Anger',
  FaceOverlay.tears => 'Tears',
  FaceOverlay.sparkles => 'Sparkles',
  FaceOverlay.question => 'Question',
};

String cycleLabel(CycleMode c) => switch (c) {
  CycleMode.off => 'Still',
  CycleMode.sequence => 'Sequence',
  CycleMode.random => 'Random',
};

String chipLabel(String? chip) => switch (chip) {
  'esp32' => 'ESP32',
  'esp32c3' => 'ESP32-C3',
  'esp32s3' => 'ESP32-S3',
  null => 'Unknown',
  _ => chip,
};

String linkStateLabel(LinkState s) => switch (s) {
  LinkState.bluetoothOff => 'Bluetooth is off',
  LinkState.unpaired => 'Not paired',
  LinkState.disconnected => 'Looking for your bunny',
  LinkState.connecting => 'Connecting',
  LinkState.connected => 'Connected',
};

/// Reply status bytes from the GATT section of ARCHITECTURE.md.
String deviceStatusMessage(int? status) => switch (status) {
  1 => 'The bunny has not accepted this phone yet. Reconnect and try again.',
  2 => 'The bunny rejected the message as malformed.',
  3 => 'That slot is not available (empty, or it is the active face).',
  4 => 'The bunny is out of space. Delete a slot or make the drawing smaller.',
  5 => 'The bunny is busy. Try again in a moment.',
  6 => 'The bunny rejected the pairing key. Scan the QR code again.',
  _ => 'The bunny reported an error${status == null ? '' : ' ($status)'}.',
};

/// A message a person can act on, for anything a link call may throw.
String errorMessage(Object e) {
  if (e is LinkException) {
    return switch (e.code) {
      'BT_OFF' => 'Bluetooth is off. Turn it on and try again.',
      'PERMISSION' => 'nixxa needs the Bluetooth (Nearby devices) and camera permissions. Allow them in Settings.',
      'NOT_FOUND' => 'No bunny found nearby. Make sure it is powered and shows its QR code, then try again.',
      'BOND_FAILED' => 'Bluetooth pairing failed. Accept the pairing request when Android asks, then try again.',
      'AUTH_FAILED' => 'The bunny rejected the key. It may have been reset: scan the QR code on its screen again.',
      'NOT_CONNECTED' => 'The bunny is not connected. Bring it closer or power it on.',
      'TIMEOUT' => 'The bunny did not answer in time. Try again.',
      'DEVICE_ERROR' => deviceStatusMessage(e.deviceStatus),
      'BAD_QR' => 'That is not a nixxa QR code. It should start with NIXXA1.',
      _ => e.message.isEmpty ? 'Something went wrong (${e.code}).' : e.message,
    };
  }
  if (e is WidgetDataException) return e.message;
  if (e is FormatException) return e.message.isEmpty ? 'The file or data is not valid.' : e.message;
  if (e is ArgumentError && e.message is String) return e.message as String;
  return 'Something went wrong: $e';
}
