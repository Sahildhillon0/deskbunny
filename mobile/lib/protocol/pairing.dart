// QR payload "NIXXA1:<ID 6 hex>:<K 32 hex>" (ARCHITECTURE.md "Pairing and security").
import 'dart:typed_data';

class PairingCode {
  final String deviceId; // 6 uppercase hex
  final Uint8List key; // 16 bytes
  const PairingCode(this.deviceId, this.key);

  static final RegExp _re = RegExp(r'^NIXXA1:([0-9A-Fa-f]{6}):([0-9A-Fa-f]{32})$');

  /// Throws FormatException for anything that isn't a valid NIXXA1 code.
  /// Surrounding whitespace (a scanner's trailing newline) is ignored; hex
  /// digits may be either case; the prefix must be exactly `NIXXA1`.
  static PairingCode parse(String qrText) {
    final m = _re.firstMatch(qrText.trim());
    if (m == null) {
      throw FormatException('Not a nixxa pairing code. Scan the QR code on the bunny\'s screen.', qrText);
    }
    final k = m.group(2)!;
    final key = Uint8List(16);
    for (var i = 0; i < 16; i++) {
      key[i] = int.parse(k.substring(i * 2, i * 2 + 2), radix: 16);
    }
    return PairingCode(m.group(1)!.toUpperCase(), key);
  }

  String get advertisedName => 'NIXXA-$deviceId';

  /// The canonical QR text (uppercase hex).
  @override
  String toString() =>
      'NIXXA1:$deviceId:${key.map((b) => b.toRadixString(16).padLeft(2, '0')).join().toUpperCase()}';
}
