import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/desktop/pages/remote_page.dart';
import 'package:flutter_hbb/desktop/widgets/remote_toolbar.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:flutter_hbb/models/session_time_model.dart';
import 'package:get/get.dart';

/// RustDesk-Velour admin monitor wall: the backend's list of active sessions
/// on the left, up to `access-wall-max-tiles` live view-only tiles on the
/// right. Each tile is a normal [RemotePage] opened with `monitoring: true`,
/// so the device forces view-only and the backend sees it flagged.
class MonitorWallPage extends StatefulWidget {
  const MonitorWallPage({Key? key}) : super(key: key);

  @override
  State<MonitorWallPage> createState() => _MonitorWallPageState();
}

/// One row of `GET /v1/sessions/active`.
class ActiveSession {
  final String sessionId;
  final String deviceId;
  final String deviceName;
  final String peerId;
  final String displayName;
  final String role;
  final String status;
  final int elapsedSeconds;
  final int? remainingSeconds;

  ActiveSession.fromJson(Map<String, dynamic> j)
      : sessionId = j['session_id']?.toString() ?? '',
        deviceId = j['device_id']?.toString() ?? '',
        deviceName = j['device_name']?.toString() ?? '',
        peerId = j['peer_id']?.toString() ?? '',
        displayName = j['display_name']?.toString() ?? '',
        role = j['role']?.toString() ?? '',
        status = j['status']?.toString() ?? '',
        elapsedSeconds = (j['elapsed_seconds'] as num?)?.toInt() ?? 0,
        remainingSeconds = (j['remaining_seconds'] as num?)?.toInt();

  static List<ActiveSession> parse(String json) {
    final data = jsonDecode(json);
    if (data is Map && data['error'] != null) {
      throw Exception(data['error'].toString());
    }
    final list = (data as Map)['sessions'] as List? ?? [];
    return list
        .map((e) => ActiveSession.fromJson(e as Map<String, dynamic>))
        .toList();
  }
}

/// Which devices are on the wall; pure so the tile limit is unit-testable.
class WallTiles {
  final int maxTiles;
  final List<String> deviceIds = [];
  WallTiles(this.maxTiles);

  bool get full => deviceIds.length >= maxTiles;
  bool contains(String deviceId) => deviceIds.contains(deviceId);

  /// True when added; false when full or already present.
  bool add(String deviceId) {
    if (full || contains(deviceId)) return false;
    deviceIds.add(deviceId);
    return true;
  }

  void remove(String deviceId) => deviceIds.remove(deviceId);

  /// Columns for a near-square grid: 1 → 1, 2 → 2, 3–4 → 2, 5–6 → 3.
  int columns() {
    final n = deviceIds.length;
    if (n <= 1) return 1;
    if (n <= 4) return 2;
    return 3;
  }
}

class _MonitorWallPageState extends State<MonitorWallPage> {
  final sessions = <ActiveSession>[].obs;
  final error = ''.obs;
  final loading = false.obs;
  final tiles = WallTiles(
      int.tryParse(bind.mainGetLocalOption(key: kOptionAccessWallMaxTiles)) ??
          6);
  final _version = 0.obs; // bumps when tiles change
  final expanded = Rxn<String>(); // device id shown alone, if any

  @override
  void initState() {
    super.initState();
    refresh();
  }

  Future<void> refresh() async {
    loading.value = true;
    error.value = '';
    try {
      final json = await bind.mainVelourActiveSessions();
      sessions.value = ActiveSession.parse(json);
    } catch (e) {
      error.value = e.toString().replaceFirst('Exception: ', '');
    } finally {
      loading.value = false;
    }
  }

  void watch(String deviceId) {
    if (tiles.add(deviceId)) {
      _version.value++;
    } else if (tiles.full) {
      showToast(translate('wall-full-tip'));
    }
  }

  void unwatch(String deviceId) {
    tiles.remove(deviceId);
    if (expanded.value == deviceId) expanded.value = null;
    _version.value++;
  }

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        SizedBox(width: 300, child: _sessionList(context)),
        const VerticalDivider(width: 1),
        Expanded(child: _grid(context)),
      ],
    );
  }

  Widget _sessionList(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 10, 8, 4),
          child: Row(children: [
            Expanded(
                child: Text(translate('Active sessions'),
                    style: const TextStyle(
                        fontSize: 15, fontWeight: FontWeight.w600))),
            Obx(() => IconButton(
                  tooltip: translate('Refresh'),
                  icon: loading.value
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(strokeWidth: 2))
                      : const Icon(Icons.refresh, size: 20),
                  onPressed: loading.value ? null : refresh,
                )),
          ]),
        ),
        Obx(() => error.value.isEmpty
            ? const SizedBox.shrink()
            : Padding(
                padding: const EdgeInsets.fromLTRB(12, 0, 12, 8),
                child: Text(error.value,
                    style: TextStyle(
                        color: Theme.of(context).colorScheme.error,
                        fontSize: 12)),
              )),
        Expanded(
          child: Obx(() {
            _version.value; // rebuild on tile changes
            if (sessions.isEmpty && error.value.isEmpty) {
              return Center(
                  child: Text(translate('No active sessions'),
                      style: TextStyle(
                          color: Theme.of(context).hintColor, fontSize: 13)));
            }
            return ListView.separated(
              itemCount: sessions.length,
              separatorBuilder: (_, __) => const Divider(height: 1),
              itemBuilder: (context, i) => _sessionRow(context, sessions[i]),
            );
          }),
        ),
        Obx(() {
          _version.value;
          return Padding(
            padding: const EdgeInsets.all(12),
            child: Text(
              '${tiles.deviceIds.length} / ${tiles.maxTiles} ${translate('tiles')}',
              style: TextStyle(
                  color: Theme.of(context).hintColor, fontSize: 12),
            ),
          );
        }),
      ],
    );
  }

  Widget _sessionRow(BuildContext context, ActiveSession s) {
    final watching = tiles.contains(s.deviceId);
    final canWatch = !watching && !tiles.full;
    final time = s.remainingSeconds != null
        ? '${translate('Time left')} ${SessionTimeModel.format(s.remainingSeconds!)}'
        : '${translate('Elapsed')} ${SessionTimeModel.format(s.elapsedSeconds)}';
    return ListTile(
      dense: true,
      title: Text(
          s.deviceName.isNotEmpty ? s.deviceName : s.deviceId,
          maxLines: 1,
          overflow: TextOverflow.ellipsis),
      subtitle: Text(
        '${s.displayName.isNotEmpty ? s.displayName : s.peerId} · ${s.role}'
        '${s.status == 'stale' ? ' · ${translate('stale')}' : ''}\n$time',
        style: const TextStyle(fontSize: 11),
      ),
      isThreeLine: true,
      trailing: watching
          ? TextButton(
              onPressed: () => unwatch(s.deviceId),
              child: Text(translate('Stop')))
          : Tooltip(
              message: canWatch ? '' : translate('wall-full-tip'),
              child: ElevatedButton(
                onPressed: canWatch ? () => watch(s.deviceId) : null,
                child: Text(translate('Watch')),
              ),
            ),
    );
  }

  Widget _grid(BuildContext context) {
    return Obx(() {
      _version.value;
      final ids = tiles.deviceIds;
      if (ids.isEmpty) {
        return Center(
          child: Text(translate('wall-empty-tip'),
              textAlign: TextAlign.center,
              style: TextStyle(color: Theme.of(context).hintColor)),
        );
      }
      final only = expanded.value;
      if (only != null && ids.contains(only)) {
        return _tile(context, only, expandedView: true);
      }
      final cols = tiles.columns();
      final rows = (ids.length / cols).ceil();
      return Column(
        children: [
          for (var r = 0; r < rows; r++)
            Expanded(
              child: Row(
                children: [
                  for (var c = 0; c < cols; c++)
                    Expanded(
                      child: r * cols + c < ids.length
                          ? _tile(context, ids[r * cols + c])
                          : const SizedBox.shrink(),
                    ),
                ],
              ),
            ),
        ],
      );
    });
  }

  Widget _tile(BuildContext context, String deviceId,
      {bool expandedView = false}) {
    final s = sessions.firstWhereOrNull((e) => e.deviceId == deviceId);
    final title = s == null
        ? deviceId
        : '${s.deviceName.isNotEmpty ? s.deviceName : deviceId} — ${s.displayName}';
    return Container(
      margin: const EdgeInsets.all(3),
      decoration: BoxDecoration(
        border: Border.all(color: Theme.of(context).dividerColor),
        borderRadius: BorderRadius.circular(4),
      ),
      child: Column(
        children: [
          Container(
            height: 28,
            color: Theme.of(context).colorScheme.surfaceVariant,
            padding: const EdgeInsets.only(left: 8),
            child: Row(children: [
              Expanded(
                  child: Text(title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(fontSize: 12))),
              IconButton(
                tooltip: expandedView
                    ? translate('Back to grid')
                    : translate('Expand'),
                iconSize: 16,
                icon: Icon(expandedView
                    ? Icons.grid_view_outlined
                    : Icons.open_in_full),
                onPressed: () =>
                    expanded.value = expandedView ? null : deviceId,
              ),
              IconButton(
                tooltip: translate('Stop'),
                iconSize: 16,
                icon: const Icon(Icons.close),
                onPressed: () => unwatch(deviceId),
              ),
            ]),
          ),
          Expanded(
            child: RemotePage(
              key: ValueKey('wall-$deviceId'),
              id: deviceId,
              toolbarState: ToolbarState(),
              monitoring: true,
            ),
          ),
        ],
      ),
    );
  }
}
