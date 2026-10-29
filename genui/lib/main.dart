import 'dart:async';
import 'dart:io';
import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:genui/genui.dart' as genui;
import 'package:logging/logging.dart';

import 'app.dart';
import 'bridge/client.dart';
import 'bridge/protocol.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  // SDK diagnostics can include component data. Never emit them.
  genui.configureLogging(level: Level.OFF, logCallback: (_, _) {});
  BridgeClient? client;
  BridgeError? error;
  try {
    client = BridgeClient(Bootstrap.fromEnvironment(Platform.environment));
  } on ProtocolFailure catch (failure) {
    error = failure.error;
  } catch (_) {
    error = BridgeError.unauthorized;
  }
  void renderingFailed() {
    // Mark terminal synchronously so an already-scheduled mounted-frame ack
    // cannot acknowledge this failed frame. Only the UI notification is
    // deferred, avoiding setState during build.
    client?.fail(BridgeError.renderFailed, deferNotification: true);
  }

  FlutterError.onError = (_) => renderingFailed();
  PlatformDispatcher.instance.onError = (_, _) {
    renderingFailed();
    return true;
  };
  ErrorWidget.builder = (_) => const Directionality(
    textDirection: TextDirection.ltr,
    child: ColoredBox(
      color: Color(0xff12171d),
      child: Padding(
        padding: EdgeInsets.all(24),
        child: Text('This task view could not be rendered safely.'),
      ),
    ),
  );
  runApp(HooterApp(client: client, initialError: error));
  if (client != null) unawaited(client.start());
}
