import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/protocol/pairing.dart';

void main() {
  const k = '00112233445566778899AABBCCDDEEFF';

  test('parses a valid code', () {
    final p = PairingCode.parse('NIXXA1:A1B2C3:$k');
    expect(p.deviceId, 'A1B2C3');
    expect(p.key.length, 16);
    expect(p.key[0], 0x00);
    expect(p.key[15], 0xFF);
    expect(p.advertisedName, 'NIXXA-A1B2C3');
    expect(p.toString(), 'NIXXA1:A1B2C3:$k');
  });

  test('lowercase hex and surrounding whitespace are accepted', () {
    final p = PairingCode.parse(' NIXXA1:a1b2c3:${k.toLowerCase()}\n');
    expect(p.deviceId, 'A1B2C3');
    expect(p.key[10], 0xAA);
  });

  test('anything else is a FormatException', () {
    for (final bad in [
      '',
      'NIXXA1:A1B2C3',
      'NIXXA2:A1B2C3:$k',
      'nixxa1:A1B2C3:$k',
      'NIXXA1:A1B2C:$k',
      'NIXXA1:A1B2C3D:$k',
      'NIXXA1:A1B2C3:${k.substring(1)}',
      'NIXXA1:A1B2C3:${k}0',
      'NIXXA1:G1B2C3:$k',
      'NIXXA1:A1B2C3:${k.replaceFirst('0', 'Z')}',
      'NIXXA1:A1B2C3:$k:',
      'XNIXXA1:A1B2C3:$k',
    ]) {
      expect(() => PairingCode.parse(bad), throwsFormatException, reason: bad);
    }
  });
}
