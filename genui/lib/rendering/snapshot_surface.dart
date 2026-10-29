import 'dart:async';

import 'package:a2ui_core/a2ui_core.dart' as core;
import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart';
import 'package:genui/genui.dart' as genui;

import '../bridge/protocol.dart';
import 'safe_catalog.dart';

/// A fresh SDK controller for each already-validated full snapshot.
/// No incremental external messages are allowed into a live controller.
final class PreparedSurface {
  PreparedSurface(this.snapshot, this._onFailure)
    : controller = genui.SurfaceController(catalogs: [createSafeCatalog()]) {
    context = _GuardedSurfaceContext(
      controller.contextFor(taskSurfaceId),
      _markFailure,
    );
    _errors = controller.onSubmit.listen(
      (_) => _markFailure(),
      onError: (Object _) => _markFailure(),
    );
    try {
      for (final message in [
        <String, Object?>{
          'version': 'v0.9',
          'createSurface': {
            'surfaceId': taskSurfaceId,
            'catalogId': safeCatalogId,
          },
        },
        <String, Object?>{
          'version': 'v0.9',
          'updateComponents': {
            'surfaceId': taskSurfaceId,
            'components': snapshot.components.values
                .map((component) => component.toA2ui())
                .toList(),
          },
        },
        <String, Object?>{
          'version': 'v0.9',
          'updateDataModel': {
            'surfaceId': taskSurfaceId,
            'path': '/',
            'value': snapshot.data,
          },
        },
      ]) {
        controller.handleMessage(core.A2uiMessage.fromJson(message));
      }
    } catch (_) {
      _markFailure();
    }
  }

  final TaskSnapshot snapshot;
  final genui.SurfaceController controller;
  final VoidCallback _onFailure;
  late final genui.SurfaceContext context;
  late final StreamSubscription<genui.ChatMessage> _errors;
  bool failed = false;
  bool disposed = false;

  void _markFailure() {
    if (disposed || failed) return;
    failed = true;
    _onFailure();
  }

  void dispose() {
    if (disposed) return;
    disposed = true;
    unawaited(_errors.cancel());
    controller.dispose();
  }
}

/// The SDK controller is an interface class and cannot be subclassed. Keep its
/// real context/models, but intercept widget errors synchronously, before ack.
final class _GuardedSurfaceContext implements genui.SurfaceContext {
  _GuardedSurfaceContext(this.delegate, this.onFailure);
  final genui.SurfaceContext delegate;
  final VoidCallback onFailure;

  @override
  String get surfaceId => delegate.surfaceId;
  @override
  ValueListenable<genui.SurfaceDefinition?> get definition =>
      delegate.definition;
  @override
  genui.DataModel get dataModel => delegate.dataModel;
  @override
  genui.Catalog? get catalog => delegate.catalog;
  @override
  void handleUiEvent(genui.UiEvent event) => onFailure();

  @override
  void reportError(Object error, StackTrace? stack) {
    onFailure();
  }
}

class SnapshotSurface extends StatefulWidget {
  const SnapshotSurface({
    super.key,
    required this.snapshot,
    required this.forward,
    required this.onApplied,
    required this.onFailure,
    this.onPrepared,
  });
  final TaskSnapshot snapshot;
  final ForwardAction forward;
  final void Function(TaskSnapshot) onApplied;
  final VoidCallback onFailure;
  final void Function(PreparedSurface)? onPrepared;

  @override
  State<SnapshotSurface> createState() => _SnapshotSurfaceState();
}

class _SnapshotSurfaceState extends State<SnapshotSurface> {
  late final PreparedSurface _prepared;

  @override
  void initState() {
    super.initState();
    _prepared = PreparedSurface(widget.snapshot, _failure);
    widget.onPrepared?.call(_prepared);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || _prepared.failed) return;
      // Allow SDK error-stream microtasks from this mounted frame to drain
      // before acknowledging. Widget errors are intercepted synchronously.
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && !_prepared.failed) widget.onApplied(widget.snapshot);
      });
      WidgetsBinding.instance.scheduleFrame();
    });
  }

  void _failure() {
    // SDK validation/error streams may fire during a widget build.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        setState(() {});
        widget.onFailure();
      }
    });
    WidgetsBinding.instance.scheduleFrame();
  }

  @override
  Widget build(BuildContext context) => _prepared.failed
      ? const Text('This task view could not be rendered safely.')
      : genui.Surface(
          key: ObjectKey(_prepared.controller),
          surfaceContext: _prepared.context,
          actionDelegate: SafeActionDelegate(widget.snapshot, widget.forward),
        );

  @override
  void dispose() {
    // Surface and BoundString widgets must unmount before disposing models
    // and ValueNotifiers. Never dispose a replaced controller in build().
    WidgetsBinding.instance.addPostFrameCallback((_) => _prepared.dispose());
    WidgetsBinding.instance.scheduleFrame();
    super.dispose();
  }
}
