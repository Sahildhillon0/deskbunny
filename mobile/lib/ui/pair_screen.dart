// 1. Pair: scan the QR code on the bunny (or type it), then the link scans,
// bonds and authenticates.
import 'package:flutter/material.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../app/labels.dart';
import '../app/theme.dart';
import '../link/device_link.dart';

class PairScreen extends StatefulWidget {
  const PairScreen({super.key});

  @override
  State<PairScreen> createState() => _PairScreenState();
}

enum _Step { idle, connecting, done }

class _PairScreenState extends State<PairScreen> {
  final _code = TextEditingController();
  _Step _step = _Step.idle;
  String? _error;
  String? _deviceId;

  @override
  void dispose() {
    _code.dispose();
    super.dispose();
  }

  static final _qrShape = RegExp(r'^NIXXA1:([0-9A-F]{6}):([0-9A-F]{32})$');

  Future<void> _scan() async {
    final text = await Navigator.of(context)
        .push<String>(MaterialPageRoute(builder: (_) => const ScannerPage(), fullscreenDialog: true));
    if (text != null && mounted) {
      _code.text = text;
      await _pair(text);
    }
  }

  Future<void> _pair(String raw) async {
    final text = raw.trim().toUpperCase();
    final m = _qrShape.firstMatch(text);
    if (m == null) {
      setState(() => _error = errorMessage(const LinkException('BAD_QR', '')));
      return;
    }
    FocusScope.of(context).unfocus();
    setState(() {
      _error = null;
      _step = _Step.connecting;
      _deviceId = m.group(1);
    });
    try {
      await context.read<AppState>().pair(text);
      if (mounted) setState(() => _step = _Step.done);
    } on Object catch (e) {
      if (mounted) {
        setState(() {
          _step = _Step.idle;
          _error = errorMessage(e);
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final busy = _step == _Step.connecting;
    final btOff = context.select<AppState, bool>((a) => a.status.state == LinkState.bluetoothOff);
    return Scaffold(
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.fromLTRB(20, 32, 20, 24),
          children: [
            const Text('nixxa', style: TextStyle(fontSize: 13, letterSpacing: 2, color: NixxaColors.text3)),
            const SizedBox(height: 8),
            const Text('Pair your bunny', style: TextStyle(fontSize: 26, fontWeight: FontWeight.w600)),
            const SizedBox(height: 8),
            const Text(
              'Power the bunny on. Its screen shows a QR code with NIXXA and its ID. Scan it to pair this phone.',
              style: TextStyle(color: NixxaColors.text2, height: 1.4),
            ),
            if (btOff) ...[
              const SizedBox(height: 16),
              const _Notice(icon: Icons.bluetooth_disabled, text: 'Bluetooth is off. Turn it on to pair.'),
            ],
            const SizedBox(height: 28),
            FilledButton.icon(
              key: const Key('pair-scan'),
              onPressed: busy ? null : _scan,
              icon: const Icon(Icons.qr_code_scanner),
              label: const Text('Scan QR code'),
            ),
            const SizedBox(height: 24),
            const Text('Or type the code', style: TextStyle(color: NixxaColors.text3, fontSize: 12)),
            const SizedBox(height: 8),
            TextField(
              key: const Key('pair-code'),
              controller: _code,
              enabled: !busy,
              autocorrect: false,
              textCapitalization: TextCapitalization.characters,
              style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
              decoration: const InputDecoration(hintText: 'NIXXA1:A1B2C3:…'),
              onSubmitted: busy ? null : _pair,
            ),
            const SizedBox(height: 8),
            OutlinedButton(
              key: const Key('pair-submit'),
              onPressed: busy ? null : () => _pair(_code.text),
              child: const Text('Pair'),
            ),
            if (_step != _Step.idle) ...[const SizedBox(height: 24), _Progress(step: _step, deviceId: _deviceId)],
            if (_error != null) ...[
              const SizedBox(height: 20),
              _Notice(key: const Key('pair-error'), icon: Icons.error_outline, text: _error!, danger: true),
            ],
          ],
        ),
      ),
    );
  }
}

class _Progress extends StatelessWidget {
  final _Step step;
  final String? deviceId;
  const _Progress({required this.step, this.deviceId});

  @override
  Widget build(BuildContext context) {
    final done = step == _Step.done;
    Widget row(String text, bool complete) => Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Row(
        children: [
          SizedBox(
            width: 20,
            height: 20,
            child: complete
                ? const Icon(Icons.check_circle, size: 20, color: NixxaColors.ok)
                : const CircularProgressIndicator(strokeWidth: 2),
          ),
          const SizedBox(width: 12),
          Expanded(child: Text(text)),
        ],
      ),
    );
    return Container(
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        color: NixxaColors.surface,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: NixxaColors.line),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          row('Find NIXXA-${deviceId ?? ''}, pair over Bluetooth and check the key', done),
          if (!done)
            const Padding(
              padding: EdgeInsets.only(left: 32, top: 4),
              child: Text(
                'If Android asks to pair with the bunny, accept. This can take up to 30 seconds.',
                style: TextStyle(fontSize: 12, color: NixxaColors.text3),
              ),
            ),
          if (done) row('Paired', true),
        ],
      ),
    );
  }
}

class _Notice extends StatelessWidget {
  final IconData icon;
  final String text;
  final bool danger;
  const _Notice({super.key, required this.icon, required this.text, this.danger = false});

  @override
  Widget build(BuildContext context) {
    final c = danger ? NixxaColors.danger : NixxaColors.warn;
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: c.withValues(alpha: 0.08),
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: c.withValues(alpha: 0.35)),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(icon, size: 18, color: c),
          const SizedBox(width: 10),
          Expanded(
            child: Text(text, style: TextStyle(color: c, height: 1.35)),
          ),
        ],
      ),
    );
  }
}

/// Full-screen camera that pops with the first NIXXA1 code it sees.
class ScannerPage extends StatefulWidget {
  const ScannerPage({super.key});

  @override
  State<ScannerPage> createState() => _ScannerPageState();
}

class _ScannerPageState extends State<ScannerPage> {
  final _controller = MobileScannerController(formats: const [BarcodeFormat.qrCode]);
  bool _done = false;
  String? _other;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _onDetect(BarcodeCapture capture) {
    if (_done) return;
    for (final b in capture.barcodes) {
      final v = b.rawValue?.trim();
      if (v == null) continue;
      if (v.toUpperCase().startsWith('NIXXA1:')) {
        _done = true;
        Navigator.of(context).pop(v);
        return;
      }
      if (_other != v) setState(() => _other = v);
    }
  }

  String _cameraError(MobileScannerException e) => switch (e.errorCode) {
    MobileScannerErrorCode.permissionDenied =>
      'nixxa needs the camera to scan the code. Allow it in Settings, or go back and type the code.',
    MobileScannerErrorCode.unsupported => 'This phone has no usable camera. Go back and type the code.',
    _ => 'The camera could not start. Go back and type the code.',
  };

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Scan the bunny')),
      body: Stack(
        fit: StackFit.expand,
        children: [
          MobileScanner(
            controller: _controller,
            onDetect: _onDetect,
            errorBuilder: (context, error) => Center(
              child: Padding(
                padding: const EdgeInsets.all(24),
                child: Text(
                  _cameraError(error),
                  textAlign: TextAlign.center,
                  style: const TextStyle(color: NixxaColors.text2),
                ),
              ),
            ),
          ),
          IgnorePointer(
            child: Center(
              child: Container(
                width: 240,
                height: 240,
                decoration: BoxDecoration(
                  border: Border.all(color: NixxaColors.accent, width: 2),
                  borderRadius: BorderRadius.circular(16),
                ),
              ),
            ),
          ),
          Positioned(
            left: 16,
            right: 16,
            bottom: 32,
            child: Text(
              _other == null
                  ? 'Point the camera at the QR code on the bunny\'s screen'
                  : 'That QR code is not a nixxa code',
              textAlign: TextAlign.center,
              style: const TextStyle(color: NixxaColors.text, shadows: [Shadow(blurRadius: 6)]),
            ),
          ),
        ],
      ),
    );
  }
}
