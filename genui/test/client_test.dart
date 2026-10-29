import 'dart:async';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:hooter_genui/bridge/client.dart';
import 'package:hooter_genui/bridge/framing.dart';
import 'package:hooter_genui/bridge/protocol.dart';

import 'fixtures.dart';

Future<void> until(bool Function() condition) async {
  final deadline = DateTime.now().add(const Duration(seconds: 3));
  while (!condition()) {
    if (DateTime.now().isAfter(deadline)) {
      fail('Timed out waiting for test state');
    }
    await Future<void>.delayed(const Duration(milliseconds: 2));
  }
}

final class TestPeer {
  TestPeer(this.directory, this.server, this.client);
  final Directory directory;
  final ServerSocket server;
  final BridgeClient client;
  Socket? host;
  StreamSubscription<Socket>? accepted;
  StreamSubscription<List<int>>? incoming;
  final messages = <Map<String, Object?>>[];

  static Future<TestPeer> create({
    Duration timeout = const Duration(milliseconds: 200),
    Duration interval = const Duration(milliseconds: 20),
  }) async {
    final directory = await Directory('/tmp').createTemp('hooter-genui-test-');
    final path = '${directory.path}/bridge.sock';
    final server = await ServerSocket.bind(
      InternetAddress(path, type: InternetAddressType.unix),
      0,
    );
    final client = BridgeClient(
      Bootstrap.fromEnvironment({
        'HOOTER_GENUI_SOCKET': '/Users/test/.flint/genui-runtime/g-aB123z/s',
        'HOOTER_GENUI_GENERATION': 'test-generation',
        'HOOTER_GENUI_CAPABILITY': 'test-capability-only',
      }),
      timeout: timeout,
      updateInterval: interval,
      connector: (_) => Socket.connect(
        InternetAddress(path, type: InternetAddressType.unix),
        0,
      ),
    );
    final peer = TestPeer(directory, server, client);
    peer.accepted = server.listen((socket) {
      peer.host = socket;
      final decoder = FrameDecoder((value) {
        peer.messages.add(object(value));
      });
      peer.incoming = socket.listen(
        decoder.add,
        onError: (Object _) {},
        onDone: () {},
      );
    });
    await client.start();
    await until(() => peer.messages.isNotEmpty);
    return peer;
  }

  void send(Map<String, Object?> json) => host!.add(encodeFrame(json));
  Future<void> ready() async {
    send(envelope('ready'));
    await until(() => client.phase == ConnectionPhase.ready);
  }

  Future<void> snapshot({int sequence = 1}) async {
    send(snapshotJson(sequence: sequence));
    await until(() => client.snapshot?.sequence == sequence);
  }

  Future<void> close() async {
    client.dispose();
    host?.destroy();
    await incoming?.cancel();
    await accepted?.cancel();
    await server.close();
    await directory.delete(recursive: true);
  }
}

/// Models a connected peer that never drains writes. Only Socket operations
/// actually used by BridgeClient are implemented.
final class StalledSocket implements Socket {
  final input = StreamController<Uint8List>();
  final blocked = Completer<void>();
  bool destroyed = false;
  @override
  void add(List<int> data) {}
  @override
  Future<void> flush() => blocked.future;
  @override
  void destroy() => destroyed = true;
  @override
  StreamSubscription<Uint8List> listen(
    void Function(Uint8List)? onData, {
    Function? onError,
    void Function()? onDone,
    bool? cancelOnError,
  }) => input.stream.listen(
    onData,
    onError: onError,
    onDone: onDone,
    cancelOnError: cancelOnError,
  );
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  test(
    'connect failure exposes only a bounded OS number, never raw details',
    () async {
      final peer = await TestPeer.create();
      addTearDown(peer.close);
      final client = BridgeClient(
        peer.client.bootstrap,
        connector: (_) async => throw const SocketException(
          'raw exception must not be displayed',
          osError: OSError('private socket path must not be displayed', 1),
        ),
      );
      addTearDown(client.dispose);
      await client.start();
      expect(client.phase, ConnectionPhase.disconnected);
      expect(client.connectErrno, 1);
      expect(client.error, isNull);
      expect(client.canAct, false);
    },
  );

  test(
    'render failure is terminal before deferred UI notification or ack',
    () async {
      final peer = await TestPeer.create();
      addTearDown(peer.close);
      await peer.ready();
      await peer.snapshot();
      var notifications = 0;
      peer.client.addListener(() => notifications++);
      peer.client.fail(BridgeError.renderFailed, deferNotification: true);
      expect(peer.client.phase, ConnectionPhase.failed);
      expect(peer.client.applied(peer.client.snapshot!), false);
      expect(peer.client.canAct, false);
      expect(notifications, 0);
      await Future<void>.delayed(Duration.zero);
      expect(notifications, 1);
      expect(peer.messages.where((m) => m['kind'] == 'ack'), isEmpty);
    },
  );

  test('write flush is bounded even when the peer never drains', () async {
    final socket = StalledSocket();
    final client = BridgeClient(
      Bootstrap.fromEnvironment({
        'HOOTER_GENUI_SOCKET': '/Users/test/.flint/genui-runtime/g-aB123z/s',
        'HOOTER_GENUI_GENERATION': 'test-generation',
        'HOOTER_GENUI_CAPABILITY': 'test-capability-only',
      }),
      timeout: const Duration(milliseconds: 40),
      connector: (_) async => socket,
    );
    addTearDown(client.dispose);
    await client.start();
    socket.input.add(encodeFrame(envelope('ready')));
    await until(() => client.phase == ConnectionPhase.ready);
    await until(() => client.phase == ConnectionPhase.disconnected);
    expect(socket.destroyed, true);
    expect(client.canAct, false);
    unawaited(socket.input.close());
  });

  test('outbound queue fails closed at a finite limit', () async {
    final peer = await TestPeer.create();
    addTearDown(peer.close);
    await peer.ready();
    await peer.snapshot();
    peer.client.applied(peer.client.snapshot!);
    for (var i = 0; i < 100; i++) {
      peer.client.sendAction(1, 'open', 'open_task');
    }
    expect(peer.client.phase, ConnectionPhase.failed);
    expect(peer.client.requestCounter, lessThanOrEqualTo(34));
    expect(peer.client.canAct, false);
  });

  test(
    'an unmounted stale frame is not acknowledged while newer state waits',
    () async {
      final peer = await TestPeer.create(
        interval: const Duration(milliseconds: 50),
      );
      addTearDown(peer.close);
      await peer.ready();
      await peer.snapshot();
      final old = peer.client.snapshot!;
      peer.send(snapshotJson(sequence: 2));
      await until(() => peer.client.lastReceivedSequence == 2);
      expect(identical(old, peer.client.snapshot), true);
      expect(peer.client.applied(old), false);
      expect(peer.messages.where((m) => m['kind'] == 'ack'), isEmpty);
      await until(() => peer.client.snapshot?.sequence == 2);
      expect(peer.client.applied(peer.client.snapshot!), true);
    },
  );

  test(
    'Unix IPC authenticates, acknowledges only applied state, forwards actions',
    () async {
      final peer = await TestPeer.create();
      addTearDown(peer.close);
      expect(peer.messages.single, {
        ...envelope('hello'),
        'token': 'test-capability-only',
      });
      expect(peer.client.canAct, false);
      await peer.ready();
      await peer.snapshot();
      expect(peer.messages.where((m) => m['kind'] == 'ack'), isEmpty);
      expect(peer.client.sendAction(1, 'open', 'open_task'), false);
      expect(peer.client.applied(peer.client.snapshot!), true);
      await until(() => peer.messages.length == 2);
      expect(peer.messages.last, {...envelope('ack'), 'sequence': 1});
      expect(peer.client.canAct, true);
      expect(peer.client.sendAction(1, 'title', 'open_task'), false);
      expect(peer.client.sendAction(1, 'open', 'stage_prompt'), false);
      expect(peer.client.sendAction(1, 'open', 'openUrl'), false);
      expect(peer.client.sendAction(0, 'open', 'open_task'), false);
      expect(peer.client.sendAction(1, 'open', 'open_task'), true);
      expect(peer.client.sendAction(1, 'stage', 'stage_prompt'), true);
      await until(() => peer.messages.length == 4);
      expect(peer.messages[2], {
        ...envelope('action'),
        'sequence': 1,
        'request': 1,
        'source': 'open',
        'name': 'open_task',
      });
      expect(peer.messages[3]['request'], 2);
      expect(
        peer.messages[3].keys,
        unorderedEquals([
          'version',
          'kind',
          'generation',
          'sequence',
          'request',
          'source',
          'name',
        ]),
      );
      expect(peer.client.requestCounter, 2);
    },
  );

  test(
    'new snapshot disables old actions immediately and coalesces to latest',
    () async {
      final peer = await TestPeer.create(
        interval: const Duration(milliseconds: 60),
      );
      addTearDown(peer.close);
      await peer.ready();
      await peer.snapshot();
      final old = peer.client.snapshot!;
      peer.client.applied(old);
      final emitted = <int>[];
      peer.client.addListener(() {
        final sequence = peer.client.snapshot?.sequence;
        if (sequence != null && (emitted.isEmpty || emitted.last != sequence)) {
          emitted.add(sequence);
        }
      });
      peer.host!.add([
        for (var i = 2; i <= 30; i++) ...encodeFrame(snapshotJson(sequence: i)),
      ]);
      await until(() => peer.client.lastReceivedSequence == 30);
      expect(peer.client.canAct, false);
      expect(peer.client.sendAction(1, 'open', 'open_task'), false);
      await until(() => peer.client.snapshot?.sequence == 30);
      expect(emitted.where((n) => n > 1), [30]);
      expect(peer.client.applied(old), false);
      peer.client.applied(peer.client.snapshot!);
      expect(peer.client.canAct, true);
      await until(
        () => peer.messages.where((m) => m['kind'] == 'ack').length == 2,
      );
      expect(
        peer.messages
            .where((m) => m['kind'] == 'ack')
            .map((m) => m['sequence']),
        [1, 30],
      );
    },
  );

  for (final kind in ['snapshot', 'shutdown', 'hello']) {
    test('rejects $kind before authenticated ready', () async {
      final peer = await TestPeer.create();
      addTearDown(peer.close);
      peer.send(kind == 'snapshot' ? snapshotJson() : envelope(kind));
      await until(() => peer.client.phase == ConnectionPhase.failed);
      expect(peer.client.canAct, false);
    });
  }

  test('handshake timeout is bounded and sanitized', () async {
    final peer = await TestPeer.create(
      timeout: const Duration(milliseconds: 60),
    );
    addTearDown(peer.close);
    await until(() => peer.client.phase == ConnectionPhase.failed);
    expect(peer.client.error, BridgeError.unauthorized);
    await until(() => peer.messages.length == 2);
    expect(peer.messages.last, {...envelope('error'), 'code': 'unauthorized'});
  });

  test('partial frame deadline does not reset for drip-fed bytes', () async {
    final peer = await TestPeer.create(
      timeout: const Duration(milliseconds: 80),
    );
    addTearDown(peer.close);
    await peer.ready();
    peer.host!.add([0]);
    await Future<void>.delayed(const Duration(milliseconds: 40));
    peer.host!.add([0]);
    await until(() => peer.client.phase == ConnectionPhase.failed);
    expect(peer.client.error, BridgeError.malformedFrame);
  });

  test(
    'quiet authenticated connection is not treated as partial frame',
    () async {
      final peer = await TestPeer.create(
        timeout: const Duration(milliseconds: 60),
      );
      addTearDown(peer.close);
      await peer.ready();
      await Future<void>.delayed(const Duration(milliseconds: 100));
      expect(peer.client.phase, ConnectionPhase.ready);
      await peer.snapshot();
    },
  );

  for (final sequence in [1, 0]) {
    test('rejects replay/non-increasing sequence $sequence', () async {
      final peer = await TestPeer.create();
      addTearDown(peer.close);
      await peer.ready();
      await peer.snapshot();
      peer.client.applied(peer.client.snapshot!);
      peer.send(snapshotJson(sequence: sequence));
      await until(() => peer.client.phase == ConnectionPhase.failed);
      expect(peer.client.error, BridgeError.invalidSnapshot);
      expect(peer.client.sendAction(1, 'open', 'open_task'), false);
    });
  }

  test('generation mismatch terminates and cannot render', () async {
    final peer = await TestPeer.create();
    addTearDown(peer.close);
    await peer.ready();
    peer.send(snapshotJson()..['generation'] = 'wrong-launch');
    await until(() => peer.client.phase == ConnectionPhase.failed);
    expect(peer.client.error, BridgeError.unauthorized);
    expect(peer.client.snapshot, isNull);
  });

  test(
    'duplicate ready is unauthorized; error and shutdown are terminal',
    () async {
      final peer = await TestPeer.create();
      addTearDown(peer.close);
      await peer.ready();
      peer.send(envelope('ready'));
      await until(() => peer.client.phase == ConnectionPhase.failed);
      expect(peer.client.error, BridgeError.unauthorized);
    },
  );

  for (final kind in ['shutdown', 'error']) {
    test('host $kind closes socket and rejects future actions', () async {
      final peer = await TestPeer.create();
      addTearDown(peer.close);
      await peer.ready();
      await peer.snapshot();
      peer.client.applied(peer.client.snapshot!);
      peer.send({
        ...envelope(kind),
        if (kind == 'error') 'code': 'unauthorized',
      });
      await until(() => peer.client.phase != ConnectionPhase.ready);
      expect(
        peer.client.phase,
        kind == 'shutdown' ? ConnectionPhase.stopped : ConnectionPhase.failed,
      );
      expect(peer.client.sendAction(1, 'open', 'open_task'), false);
    });
  }

  for (final truncated in [false, true]) {
    test(
      'EOF ${truncated ? 'with truncation fails' : 'disconnects'}',
      () async {
        final peer = await TestPeer.create();
        addTearDown(peer.close);
        await peer.ready();
        await peer.snapshot();
        peer.client.applied(peer.client.snapshot!);
        if (truncated) {
          peer.host!.add([0, 0, 0, 12, 123]);
          await peer.host!.flush();
        }
        await peer.host!.close();
        await until(() => peer.client.phase != ConnectionPhase.ready);
        expect(
          peer.client.phase,
          truncated ? ConnectionPhase.failed : ConnectionPhase.disconnected,
        );
        expect(
          peer.client.error,
          truncated ? BridgeError.malformedFrame : null,
        );
        expect(peer.client.canAct, false);
      },
    );
  }

  test(
    'malicious full snapshot never replaces successfully applied state',
    () async {
      final peer = await TestPeer.create();
      addTearDown(peer.close);
      await peer.ready();
      await peer.snapshot();
      final old = peer.client.snapshot!;
      peer.client.applied(old);
      final malicious = snapshotJson(sequence: 2);
      componentsOf(malicious)[6]['action'] = {
        'functionCall': {'call': 'openUrl'},
      };
      peer.send(malicious);
      await until(() => peer.client.phase == ConnectionPhase.failed);
      expect(identical(peer.client.snapshot, old), true);
      expect(peer.client.appliedSequence, 1);
      await until(() => peer.messages.last['kind'] == 'error');
      expect(peer.messages.last, {
        ...envelope('error'),
        'code': 'invalid_snapshot',
      });
    },
  );

  test('teardown cancels pending updates and disposal is idempotent', () async {
    final peer = await TestPeer.create(
      interval: const Duration(milliseconds: 100),
    );
    addTearDown(peer.close);
    await peer.ready();
    peer.send(snapshotJson());
    await until(() => peer.client.lastReceivedSequence == 1);
    peer.client.dispose();
    peer.client.dispose();
    await Future<void>.delayed(const Duration(milliseconds: 150));
    expect(peer.client.snapshot, isNull);
    expect(peer.client.phase, ConnectionPhase.stopped);
    expect(peer.client.sendAction(1, 'open', 'open_task'), false);
  });

  test('bounded connect timeout destroys a late socket', () async {
    final peer = await TestPeer.create();
    addTearDown(peer.close);
    final completer = Completer<Socket>();
    final client = BridgeClient(
      peer.client.bootstrap,
      timeout: const Duration(milliseconds: 20),
      connector: (_) => completer.future,
    );
    addTearDown(client.dispose);
    await client.start();
    expect(client.phase, ConnectionPhase.disconnected);
    completer.complete(peer.host!);
    await Future<void>.delayed(const Duration(milliseconds: 10));
    expect(client.canAct, false);
  });
}
