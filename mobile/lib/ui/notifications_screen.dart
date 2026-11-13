// 6. Notifications: master switch, notification access, per-app rules with a
// mood, quiet hours, DND, calls, card time, default mood, test.
// On iOS the bunny reads notifications itself (ANCS): access means "Share
// System Notifications" in Settings > Bluetooth, and apps can be added by
// bundle ID (ARCHITECTURE.md "iOS (phase 3)").
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../app/labels.dart';
import '../app/theme.dart';
import '../link/device_link.dart';
import '../model/face.dart';
import 'widgets/common.dart';

const int maxRules = 32;
const int defaultMoodIndex = 0xFF;

bool get _isIOS => defaultTargetPlatform == TargetPlatform.iOS;

/// A bundle ID the bunny can match: printable ASCII without spaces, <= 64.
bool validBundleId(String id) =>
    id.isNotEmpty && id.length <= 64 && id.codeUnits.every((u) => u > 0x20 && u < 0x7F);

NotifConfig copyConfig(
  NotifConfig c, {
  bool? enabled,
  int? cardSecs,
  int? defaultMood,
  int? quietStartMin,
  int? quietEndMin,
  bool? callsInQuiet,
  bool? followDnd,
  bool? calls,
  List<NotifRule>? rules,
}) => NotifConfig(
  enabled: enabled ?? c.enabled,
  cardSecs: cardSecs ?? c.cardSecs,
  defaultMood: defaultMood ?? c.defaultMood,
  quietStartMin: quietStartMin ?? c.quietStartMin,
  quietEndMin: quietEndMin ?? c.quietEndMin,
  callsInQuiet: callsInQuiet ?? c.callsInQuiet,
  followDnd: followDnd ?? c.followDnd,
  calls: calls ?? c.calls,
  rules: rules ?? c.rules,
);

class NotificationsScreen extends StatefulWidget {
  const NotificationsScreen({super.key});

  @override
  State<NotificationsScreen> createState() => _NotificationsScreenState();
}

class _NotificationsScreenState extends State<NotificationsScreen> with WidgetsBindingObserver {
  NotifConfig? _config;
  bool? _access;
  List<AppInfo>? _apps;
  String? _loadError;
  String _query = '';
  bool _showAll = false;
  final _bundleId = TextEditingController();

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _load();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _bundleId.dispose();
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    // Coming back from the notification-access settings page.
    if (state == AppLifecycleState.resumed) _checkAccess();
  }

  Future<void> _checkAccess() async {
    try {
      final a = await context.read<AppState>().link.hasNotificationAccess();
      if (mounted) setState(() => _access = a);
    } on Object {
      // keep the last answer
    }
  }

  Future<void> _load() async {
    final app = context.read<AppState>();
    setState(() => _loadError = null);
    try {
      final c = await app.loadNotifConfig();
      if (mounted) setState(() => _config = c);
    } on Object catch (e) {
      if (mounted) setState(() => _loadError = errorMessage(e));
    }
    await _checkAccess();
    try {
      final apps = await app.link.listApps();
      if (mounted) setState(() => _apps = apps);
    } on Object {
      if (mounted) setState(() => _apps = const []);
    }
  }

  Future<void> _save(NotifConfig next) async {
    final prev = _config;
    setState(() => _config = next);
    final ok = await guard(context, () => context.read<AppState>().saveNotifConfig(next));
    if (!ok && mounted) setState(() => _config = prev);
  }

  NotifRule? _rule(String appId) {
    for (final r in _config?.rules ?? const <NotifRule>[]) {
      if (r.appId == appId) return r;
    }
    return null;
  }

  void _setRule(String appId, {bool? show, int? mood}) {
    final c = _config!;
    final old = _rule(appId);
    final next = NotifRule(appId, show: show ?? old?.show ?? true, mood: mood ?? old?.mood ?? defaultMoodIndex);
    final rules = [...c.rules.where((r) => r.appId != appId)];
    final keep = next.show || next.mood != defaultMoodIndex;
    if (keep) {
      if (old == null && rules.length >= maxRules) {
        showMessage(context, 'The bunny holds up to $maxRules app rules. Turn another app off first.', error: true);
        return;
      }
      final i = c.rules.indexWhere((r) => r.appId == appId);
      rules.insert(i < 0 ? rules.length : i, next);
    }
    _save(copyConfig(c, rules: rules));
  }

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();
    final c = _config;
    return Scaffold(
      appBar: AppBar(title: const Text('Notifications')),
      body: c == null
          ? Center(
              child: _loadError == null
                  ? const CircularProgressIndicator()
                  : Padding(
                      padding: const EdgeInsets.all(24),
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Text(
                            _loadError!,
                            textAlign: TextAlign.center,
                            style: const TextStyle(color: NixxaColors.text2),
                          ),
                          const SizedBox(height: 12),
                          OutlinedButton(onPressed: _load, child: const Text('Try again')),
                        ],
                      ),
                    ),
            )
          : ListView(
              padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
              children: [
                SwitchRow(
                  key: const Key('notif-enabled'),
                  label: 'Show notifications on the bunny',
                  value: c.enabled,
                  onChanged: (v) => _save(copyConfig(c, enabled: v)),
                ),
                if (_access == false) _accessCard(app),
                if (_access == true)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 4),
                    child: Row(
                      children: [
                        Icon(Icons.check_circle, size: 16, color: NixxaColors.ok),
                        SizedBox(width: 8),
                        Flexible(
                          child: Text(
                            _isIOS ? 'iOS shares notifications with the bunny' : 'Notification access allowed',
                            style: const TextStyle(fontSize: 12, color: NixxaColors.text2),
                          ),
                        ),
                      ],
                    ),
                  ),
                const SectionLabel('Card'),
                SliderRow(
                  label: 'Show for',
                  value: c.cardSecs,
                  min: 2,
                  max: 30,
                  format: (v) => '$v s',
                  onChanged: (v) => setState(() => _config = copyConfig(c, cardSecs: v)),
                  onChangeEnd: (_) => _save(_config!),
                ),
                _MoodField(
                  key: const Key('notif-default-mood'),
                  label: 'Default mood',
                  value: c.defaultMood,
                  allowDefault: false,
                  onChanged: (v) => _save(copyConfig(c, defaultMood: v)),
                ),
                const SectionLabel('Calls and quiet time'),
                SwitchRow(
                  key: const Key('notif-dnd'),
                  label: 'Do Not Disturb',
                  hint: app.connected
                      ? 'Silences the bunny until you turn it off'
                      : 'Connect to the bunny to change this',
                  value: app.device?.dnd ?? false,
                  onChanged: app.connected && app.device != null ? (v) => guard(context, () => app.setDnd(v)) : null,
                ),
                SwitchRow(
                  label: 'Incoming calls',
                  hint: 'Ring on the bunny with the caller\'s name',
                  value: c.calls,
                  onChanged: (v) => _save(copyConfig(c, calls: v)),
                ),
                SwitchRow(
                  label: 'Follow Do Not Disturb',
                  hint: _isIOS
                      ? 'Nothing shows while a Focus is on (checked while nixxa is open)'
                      : 'Nothing shows while the phone is on DND',
                  value: c.followDnd,
                  onChanged: (v) => _save(copyConfig(c, followDnd: v)),
                ),
                SwitchRow(
                  key: const Key('notif-quiet'),
                  label: 'Quiet hours',
                  value: c.quietStartMin != c.quietEndMin,
                  onChanged: (v) => _save(copyConfig(c, quietStartMin: v ? 22 * 60 : 0, quietEndMin: v ? 7 * 60 : 0)),
                ),
                if (c.quietStartMin != c.quietEndMin) ...[
                  Row(
                    children: [
                      Expanded(
                        child: _TimeButton(
                          label: 'From',
                          minutes: c.quietStartMin,
                          onChanged: (m) => _quiet(c, start: m),
                        ),
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: _TimeButton(
                          label: 'To',
                          minutes: c.quietEndMin,
                          onChanged: (m) => _quiet(c, end: m),
                        ),
                      ),
                    ],
                  ),
                  SwitchRow(
                    label: 'Calls ring during quiet hours',
                    value: c.callsInQuiet,
                    onChanged: (v) => _save(copyConfig(c, callsInQuiet: v)),
                  ),
                ],
                const SizedBox(height: 12),
                // Collapsed by default: the list is long, and most people only
                // open it to switch one app on.
                Theme(
                  data: Theme.of(context).copyWith(dividerColor: Colors.transparent),
                  child: ExpansionTile(
                    key: const Key('notif-apps'),
                    tilePadding: EdgeInsets.zero,
                    childrenPadding: EdgeInsets.zero,
                    title: const Text('Apps', style: TextStyle(fontWeight: FontWeight.w600)),
                    subtitle: Text(
                      _appsSummary(c),
                      style: const TextStyle(fontSize: 12, color: NixxaColors.text3),
                    ),
                    children: [
                      const Align(
                        alignment: Alignment.centerLeft,
                        child: Text(
                          'Only the apps switched on here show on the bunny.',
                          style: TextStyle(fontSize: 12, color: NixxaColors.text3),
                        ),
                      ),
                      const SizedBox(height: 8),
                      TextField(
                        key: const Key('notif-search'),
                        decoration: const InputDecoration(
                          prefixIcon: Icon(Icons.search, size: 20),
                          hintText: 'Search apps',
                        ),
                        onChanged: (v) => setState(() => _query = v.trim().toLowerCase()),
                      ),
                      const SizedBox(height: 8),
                      if (_isIOS) _bundleIdRow(),
                      ..._appRows(c),
                    ],
                  ),
                ),
                const SizedBox(height: 16),
                OutlinedButton.icon(
                  key: const Key('notif-test'),
                  onPressed: app.connected
                      ? () => guard(context, app.link.sendTestNotification, success: 'Sent a test notification')
                      : null,
                  icon: const Icon(Icons.notifications_active_outlined),
                  label: Text(app.connected ? 'Send a test notification' : 'Connect to send a test'),
                ),
              ],
            ),
    );
  }

  static String _appsSummary(NotifConfig c) {
    final on = c.rules.where((r) => r.show).length;
    return switch (on) {
      0 => 'None switched on',
      1 => '1 app switched on',
      _ => '$on apps switched on',
    };
  }

  void _quiet(NotifConfig c, {int? start, int? end}) {
    final s = start ?? c.quietStartMin, e = end ?? c.quietEndMin;
    if (s == e) {
      showMessage(context, 'Start and end can\'t be the same time.', error: true);
      return;
    }
    _save(copyConfig(c, quietStartMin: s, quietEndMin: e));
  }

  Widget _accessCard(AppState app) => Container(
    margin: const EdgeInsets.only(bottom: 4),
    padding: const EdgeInsets.all(12),
    decoration: BoxDecoration(
      color: NixxaColors.warn.withValues(alpha: 0.08),
      borderRadius: BorderRadius.circular(8),
      border: Border.all(color: NixxaColors.warn.withValues(alpha: 0.35)),
    ),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          _isIOS ? 'iOS isn\'t sharing notifications with the bunny' : 'nixxa can\'t read notifications yet',
          style: const TextStyle(color: NixxaColors.warn, fontWeight: FontWeight.w600),
        ),
        const SizedBox(height: 4),
        Text(
          _isIOS
              ? 'The bunny reads notifications straight from iOS, even when nixxa is closed. '
                    'In Settings > Bluetooth, tap (i) next to ${_bunnyName(app)} and turn on '
                    'Share System Notifications. The bunny needs to be connected for this check.'
              : 'Android asks you to allow notification access for nixxa in Settings.',
          style: const TextStyle(fontSize: 13, color: NixxaColors.text2),
        ),
        const SizedBox(height: 8),
        OutlinedButton(
          key: const Key('notif-access'),
          onPressed: () async {
            await guard(context, app.link.openNotificationAccessSettings);
            await _checkAccess();
          },
          child: Text(_isIOS ? 'Open Settings' : 'Allow notification access'),
        ),
      ],
    ),
  );

  static String _bunnyName(AppState app) {
    final id = app.status.deviceId;
    return id == null ? 'your bunny' : 'NIXXA-$id';
  }

  void _addBundleId() {
    final id = _bundleId.text.trim();
    if (!validBundleId(id)) {
      showMessage(context, 'Enter a bundle ID like com.example.app (letters, digits and dots, up to 64).', error: true);
      return;
    }
    if (_rule(id)?.show == true) {
      showMessage(context, '$id is already on.');
      return;
    }
    _bundleId.clear();
    FocusScope.of(context).unfocus();
    _setRule(id, show: true);
  }

  /// iOS: apps the bunny hasn't seen yet can be added by bundle ID.
  Widget _bundleIdRow() => Padding(
    padding: const EdgeInsets.only(bottom: 8),
    child: Row(
      children: [
        Expanded(
          child: TextField(
            key: const Key('notif-bundle-id'),
            controller: _bundleId,
            autocorrect: false,
            enableSuggestions: false,
            keyboardType: TextInputType.url,
            decoration: const InputDecoration(
              prefixIcon: Icon(Icons.add_box_outlined, size: 20),
              hintText: 'Add by bundle ID, e.g. com.example.app',
            ),
            onSubmitted: (_) => _addBundleId(),
          ),
        ),
        const SizedBox(width: 8),
        OutlinedButton(key: const Key('notif-bundle-add'), onPressed: _addBundleId, child: const Text('Add')),
      ],
    ),
  );

  List<Widget> _appRows(NotifConfig c) {
    final apps = _apps;
    if (apps == null) {
      return const [
        Padding(
          padding: EdgeInsets.all(16),
          child: Center(child: CircularProgressIndicator()),
        ),
      ];
    }
    final known = {for (final a in apps) a.appId};
    final all = [
      // Apps with a rule first, then the rest by label.
      ...apps.where((a) => _rule(a.appId) != null),
      ...[
        for (final r in c.rules)
          if (!known.contains(r.appId)) AppInfo(r.appId, r.appId, null),
      ],
      ...apps.where((a) => _rule(a.appId) == null),
    ];
    final filtered = _query.isEmpty
        ? all
        : all.where((a) => a.label.toLowerCase().contains(_query) || a.appId.toLowerCase().contains(_query)).toList();
    final limit = _showAll || _query.isNotEmpty ? filtered.length : 40;
    return [
      if (filtered.isEmpty)
        const Padding(
          padding: EdgeInsets.all(12),
          child: Text('No apps match.', style: TextStyle(color: NixxaColors.text3)),
        ),
      for (final a in filtered.take(limit)) _AppRow(app: a, rule: _rule(a.appId), onChanged: _setRule),
      if (filtered.length > limit)
        TextButton(onPressed: () => setState(() => _showAll = true), child: Text('Show all ${filtered.length} apps')),
    ];
  }
}

class _AppRow extends StatelessWidget {
  final AppInfo app;
  final NotifRule? rule;
  final void Function(String appId, {bool? show, int? mood}) onChanged;
  const _AppRow({required this.app, required this.rule, required this.onChanged});

  @override
  Widget build(BuildContext context) {
    final on = rule?.show ?? false;
    final icon = app.iconPng;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        children: [
          SizedBox(
            width: 32,
            height: 32,
            child: icon != null
                ? ClipRRect(borderRadius: BorderRadius.circular(6), child: Image.memory(icon, gaplessPlayback: true))
                : Container(
                    decoration: BoxDecoration(color: NixxaColors.surface3, borderRadius: BorderRadius.circular(6)),
                    alignment: Alignment.center,
                    child: Text(
                      app.label.isEmpty ? '?' : app.label[0].toUpperCase(),
                      style: const TextStyle(color: NixxaColors.text2),
                    ),
                  ),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(app.label, maxLines: 1, overflow: TextOverflow.ellipsis),
                if (on)
                  _MoodDropdown(
                    key: Key('rule-mood-${app.appId}'),
                    value: rule?.mood ?? defaultMoodIndex,
                    allowDefault: true,
                    dense: true,
                    onChanged: (m) => onChanged(app.appId, mood: m),
                  )
                else
                  Text(
                    app.appId,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontSize: 11, color: NixxaColors.text3),
                  ),
              ],
            ),
          ),
          Switch(
            key: Key('rule-${app.appId}'),
            value: on,
            onChanged: (v) => onChanged(app.appId, show: v),
          ),
        ],
      ),
    );
  }
}

class _MoodField extends StatelessWidget {
  final String label;
  final int value;
  final bool allowDefault;
  final ValueChanged<int> onChanged;
  const _MoodField({
    super.key,
    required this.label,
    required this.value,
    required this.allowDefault,
    required this.onChanged,
  });

  @override
  Widget build(BuildContext context) => Row(
    children: [
      SizedBox(
        width: 108,
        child: Text(label, style: const TextStyle(color: NixxaColors.text2, fontSize: 13)),
      ),
      Expanded(
        child: _MoodDropdown(value: value, allowDefault: allowDefault, onChanged: onChanged),
      ),
    ],
  );
}

class _MoodDropdown extends StatelessWidget {
  final int value;
  final bool allowDefault;
  final bool dense;
  final ValueChanged<int> onChanged;
  const _MoodDropdown({
    super.key,
    required this.value,
    required this.allowDefault,
    required this.onChanged,
    this.dense = false,
  });

  @override
  Widget build(BuildContext context) {
    final valid = value == defaultMoodIndex ? allowDefault : value >= 0 && value < Mood.values.length;
    return DropdownButton<int>(
      value: valid ? value : (allowDefault ? defaultMoodIndex : Mood.surprised.index),
      isDense: dense,
      isExpanded: !dense,
      underline: const SizedBox.shrink(),
      dropdownColor: NixxaColors.surface2,
      style: TextStyle(fontSize: dense ? 12 : 14, color: dense ? NixxaColors.text2 : NixxaColors.text),
      items: [
        if (allowDefault) const DropdownMenuItem(value: defaultMoodIndex, child: Text('Default mood')),
        for (final m in Mood.values) DropdownMenuItem(value: m.index, child: Text(moodLabel(m))),
      ],
      onChanged: (v) {
        if (v != null) onChanged(v);
      },
    );
  }
}

class _TimeButton extends StatelessWidget {
  final String label;
  final int minutes;
  final ValueChanged<int> onChanged;
  const _TimeButton({required this.label, required this.minutes, required this.onChanged});

  @override
  Widget build(BuildContext context) {
    final t = TimeOfDay(hour: (minutes ~/ 60) % 24, minute: minutes % 60);
    return OutlinedButton(
      onPressed: () async {
        final picked = await showTimePicker(context: context, initialTime: t, helpText: 'Quiet hours: $label');
        if (picked != null) onChanged(picked.hour * 60 + picked.minute);
      },
      child: Text('$label  ${t.format(context)}'),
    );
  }
}
