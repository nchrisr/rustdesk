import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/session_time_model.dart';
import 'package:get/get.dart';

/// Top-right badge on a remote view: elapsed time while access control is
/// active, plus the remaining-time countdown once it is under the threshold
/// (RustDesk-Velour). Never intercepts input.
class SessionCountdown extends StatelessWidget {
  final SessionTimeModel model;
  const SessionCountdown({Key? key, required this.model}) : super(key: key);

  @override
  Widget build(BuildContext context) {
    return Positioned(
      top: 8,
      right: 12,
      child: IgnorePointer(
        child: Obx(() {
          if (!model.active) return const SizedBox.shrink();
          final show = model.showCountdown;
          final red = show && model.isRed;
          final bg = red ? Colors.red.shade700 : Colors.black87;
          return Container(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 5),
            decoration: BoxDecoration(
              color: bg.withOpacity(0.85),
              borderRadius: BorderRadius.circular(6),
            ),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(Icons.timer_outlined, size: 14, color: Colors.white70),
                const SizedBox(width: 6),
                Text(
                  show
                      ? '${translate('Time left')} ${SessionTimeModel.format(model.remaining.value ?? 0)}'
                      : '${translate('Elapsed')} ${SessionTimeModel.format(model.elapsed.value)}',
                  style: TextStyle(
                    color: Colors.white,
                    fontSize: 12,
                    fontWeight: show ? FontWeight.w600 : FontWeight.normal,
                    fontFeatures: const [FontFeature.tabularFigures()],
                  ),
                ),
              ],
            ),
          );
        }),
      ),
    );
  }
}
