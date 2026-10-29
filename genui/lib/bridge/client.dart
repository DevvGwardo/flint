import 'dart:async';
import 'dart:collection';
import 'dart:io';

import 'package:flutter/foundation.dart';

import 'framing.dart';
import 'protocol.dart';

enum ConnectionPhase {
  starting,
  authenticating,
  ready,
  disconnected,
  failed,
  stopped,
}

/// Bootstrap is exclusively environment-based. No credentials or model API.
final class Bootstrap {
  const Bootstrap._(this.socketPath, this.generation, this._capability);
  final String socketPath;
  final String generation;
  final String _capability;

  static Bootstrap fromEnvironment(Map<String, String> environment) {
    final socket = environment['HOOTER_GENUI_SOCKET'];
    final generation = environment['HOOTER_GENUI_GENERATION'];
    final capability = environment['HOOTER_GENUI_CAPABILITY'];
    // HOME may be a sandbox container. Validate the actual socket location by
    // its stable host-owned suffix, not by expanding the child's HOME.
    if (socket == null ||
        !socket.startsWith('/') ||
        !RegExp(
          r'^/[^\\\x00-\x1f]*/\.flint/genui-runtime/g-[A-Za-z0-9]{6}/s$',
        ).hasMatch(socket) ||
        socket.split('/').any((part) => part == '.' || part == '..') ||
        !validGeneration(generation) ||
        capability == null ||
        capability.isEmpty ||
        capability.length > maxFrameBytes) {
      throw const ProtocolFailure(BridgeError.unauthorized);
    }
    // Capabilities are opaque strings, not identifiers. Apply the actual wire
    // budget, including UTF-8 and JSON escaping, instead of an invented format.
    encodeFrame({
      'version': 1,
      'kind': 'hello',
      'generation': generation,
      'token': capability,
    });
    return Bootstrap._(socket, generation as String, capability);
  }
}

typedef SocketConnector = Future<Socket> Function(String path);

/// Owns IPC only. The UI must explicitly call [applied] after a successful
/// SDK/widget frame. Receiving a snapshot alone never acknowledges it.
final class BridgeClient extends ChangeNotifier {
  BridgeClient(
    this.bootstrap, {
    this.timeout = const Duration(seconds: 5),
    this.updateInterval = const Duration(microseconds: 33334),
    SocketConnector? connector,
  }) : _connector = connector ?? _connectUnix {
    _decoder = FrameDecoder(_receive);
  }

  final Bootstrap bootstrap;
  final Duration timeout;
  final Duration updateInterval;
  final SocketConnector _connector;
  late final FrameDecoder _decoder;
  Socket? _socket;
  StreamSubscription<Uint8List>? _subscription;
  Timer? _handshakeTimer;
  Timer? _frameTimer;
  Timer? _coalesceTimer;
  final Queue<Uint8List> _writes = Queue();
  var _writing = false;
  var _disposed = false;
  var _terminal = false;
  var _started = false;
  var _lastReceived = 0;
  var _applied = 0;
  var _request = 0;
  TaskSnapshot? _pending;
  TaskSnapshot? snapshot;
  ConnectionPhase phase = ConnectionPhase.starting;
  BridgeError? error;
  int? _connectErrno;

  bool get canAct =>
      !_terminal &&
      phase == ConnectionPhase.ready &&
      snapshot != null &&
      snapshot!.sequence == _applied &&
      _applied == _lastReceived;
  int get lastReceivedSequence => _lastReceived;
  int get appliedSequence => _applied;
  int get requestCounter => _request;

  /// Numeric OS status only; never expose the exception message or socket path.
  int? get connectErrno => _connectErrno;

  static Future<Socket> _connectUnix(String path) =>
      Socket.connect(InternetAddress(path, type: InternetAddressType.unix), 0);

  Future<void> start() async {
    if (_started || _terminal) return;
    _started = true;
    try {
      // Also destroy a late connection after timeout/disposal.
      final future = _connector(bootstrap.socketPath);
      unawaited(
        future.then((socket) {
          if (_terminal) socket.destroy();
        }, onError: (Object _) {}),
      );
      final socket = await future.timeout(timeout);
      if (_terminal) {
        socket.destroy();
        return;
      }
      _socket = socket;
      phase = ConnectionPhase.authenticating;
      _handshakeTimer = Timer(timeout, () => fail(BridgeError.unauthorized));
      _subscription = socket.listen(
        _read,
        onError: (Object _) => _decoder.hasPartialFrame
            ? fail(BridgeError.malformedFrame)
            : _disconnect(),
        onDone: _eof,
        cancelOnError: true,
      );
      _enqueue({..._envelope('hello'), 'token': bootstrap._capability});
      _notify();
    } catch (failure) {
      if (failure is SocketException) {
        final code = failure.osError?.errorCode;
        if (code != null && code >= 0 && code <= 65535) _connectErrno = code;
      }
      if (!_terminal) _disconnect();
    }
  }

  Map<String, Object?> _envelope(String kind) => {
    'version': 1,
    'kind': kind,
    'generation': bootstrap.generation,
  };

  void _read(Uint8List bytes) {
    if (_terminal) return;
    final completedBefore = _decoder.completedFrames;
    try {
      _decoder.add(bytes);
      if (_terminal) return;
      // This is a whole-frame deadline, not a reset-on-each-byte timeout.
      if (!_decoder.hasPartialFrame) {
        _frameTimer?.cancel();
        _frameTimer = null;
      } else if (_frameTimer == null ||
          completedBefore != _decoder.completedFrames) {
        _frameTimer?.cancel();
        _frameTimer = Timer(timeout, () => fail(BridgeError.malformedFrame));
      }
    } on ProtocolFailure catch (failure) {
      fail(failure.error);
    } catch (_) {
      fail(BridgeError.malformedFrame);
    }
  }

  void _receive(Object? value) {
    if (_terminal) return;
    final message = HostMessage.parse(value, bootstrap.generation);
    if (message.kind == HostMessageKind.error) {
      _terminate(ConnectionPhase.failed, code: message.error);
      return;
    }
    if (phase == ConnectionPhase.authenticating) {
      if (message.kind != HostMessageKind.ready) {
        throw const ProtocolFailure(BridgeError.unauthorized);
      }
      _handshakeTimer?.cancel();
      phase = ConnectionPhase.ready;
      _notify();
      return;
    }
    if (phase != ConnectionPhase.ready ||
        message.kind == HostMessageKind.ready) {
      throw const ProtocolFailure(BridgeError.unauthorized);
    }
    switch (message.kind) {
      case HostMessageKind.snapshot:
        final next = message.snapshot!;
        if (next.sequence <= _lastReceived) {
          throw const ProtocolFailure(BridgeError.invalidSnapshot);
        }
        _lastReceived = next.sequence;
        _pending = next;
        // One replaceable pending update; never queue snapshots or controllers.
        _coalesceTimer ??= Timer(updateInterval, () {
          _coalesceTimer = null;
          if (_terminal) return;
          snapshot = _pending;
          _pending = null;
          _notify();
        });
        // Immediately disables actions while newer state is in flight.
        _notify();
      case HostMessageKind.shutdown:
        _terminate(ConnectionPhase.stopped);
      case HostMessageKind.ready:
      case HostMessageKind.error:
        throw const ProtocolFailure(BridgeError.malformedFrame);
    }
  }

  bool applied(TaskSnapshot rendered) {
    if (_terminal ||
        phase != ConnectionPhase.ready ||
        !identical(rendered, snapshot) ||
        rendered.sequence != _lastReceived ||
        rendered.sequence <= _applied) {
      return false;
    }
    _applied = rendered.sequence;
    _enqueue({..._envelope('ack'), 'sequence': _applied});
    _notify();
    return !_terminal;
  }

  bool sendAction(int sequence, String source, String name) {
    if (!canAct || sequence != _applied || _request >= maxSafeInteger) {
      return false;
    }
    final component = snapshot!.components[source];
    if (component?.kind != ComponentKind.button ||
        component?.action?.wireName != name) {
      return false;
    }
    _request++;
    _enqueue({
      ..._envelope('action'),
      'sequence': _applied,
      'request': _request,
      'source': source,
      'name': name,
    });
    return !_terminal;
  }

  void _enqueue(Map<String, Object?> value) {
    if (_terminal) return;
    if (_writes.length >= 32) {
      fail(BridgeError.malformedFrame);
      return;
    }
    _writes.add(encodeFrame(value));
    if (!_writing) unawaited(_drain());
  }

  Future<void> _drain() async {
    _writing = true;
    try {
      while (!_terminal && _writes.isNotEmpty) {
        _socket!.add(_writes.removeFirst());
        await _socket!.flush().timeout(timeout);
      }
    } catch (_) {
      if (!_terminal) _disconnect();
    } finally {
      _writing = false;
    }
  }

  void _eof() {
    if (_terminal) return;
    try {
      _decoder.finish();
      _disconnect();
    } on ProtocolFailure catch (failure) {
      fail(failure.error);
    }
  }

  void _disconnect() => _terminate(ConnectionPhase.disconnected);

  void fail(BridgeError code, {bool deferNotification = false}) {
    if (_terminal) return;
    // A single fixed-size diagnostic, never an echo of the rejected frame.
    // Terminal flag disables events immediately, before any asynchronous IO.
    final socket = _socket;
    _terminate(
      ConnectionPhase.failed,
      code: code,
      keepSocket: socket != null,
      notify: !deferNotification,
    );
    if (deferNotification) scheduleMicrotask(_notify);
    if (socket != null) {
      unawaited(() async {
        try {
          socket.add(encodeFrame({..._envelope('error'), 'code': code.code}));
          await socket.flush().timeout(timeout);
        } catch (_) {
          // Deliberately sanitized and silent.
        } finally {
          socket.destroy();
        }
      }());
    }
  }

  void _terminate(
    ConnectionPhase state, {
    BridgeError? code,
    bool keepSocket = false,
    bool notify = true,
  }) {
    if (_terminal) return;
    _terminal = true;
    phase = state;
    error = code;
    _handshakeTimer?.cancel();
    _frameTimer?.cancel();
    _coalesceTimer?.cancel();
    _pending = null;
    _writes.clear();
    _decoder.close();
    unawaited(_subscription?.cancel());
    if (!keepSocket) _socket?.destroy();
    if (notify) _notify();
  }

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    if (_disposed) return;
    _disposed = true;
    if (!_terminal) _terminate(ConnectionPhase.stopped);
    _socket?.destroy();
    super.dispose();
  }
}
