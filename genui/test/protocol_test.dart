import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:hooter_genui/bridge/client.dart';
import 'package:hooter_genui/bridge/protocol.dart';

import 'fixtures.dart';

final invalidSnapshot = throwsA(
  isA<ProtocolFailure>().having(
    (e) => e.error,
    'error',
    BridgeError.invalidSnapshot,
  ),
);
final malformed = throwsA(
  isA<ProtocolFailure>().having(
    (e) => e.error,
    'error',
    BridgeError.malformedFrame,
  ),
);

void main() {
  test('complete snapshot becomes immutable typed state', () {
    final snapshot = fixture();
    expect(snapshot.sequence, 1);
    expect(snapshot.task!.outcome, TaskOutcome.needsReview);
    expect(snapshot.components['context']!.text, isA<BoundText>());
    expect(
      snapshot.components['context']!.text!.resolve(snapshot.data),
      'Bound task context',
    );
    expect(() => snapshot.data['new'] = 'no', throwsUnsupportedError);
    expect(() => snapshot.components.clear(), throwsUnsupportedError);
    expect(
      () => snapshot.components['root']!.children.clear(),
      throwsUnsupportedError,
    );
  });

  test('all outcomes and null task are host-owned exact enums', () {
    for (final outcome in [
      'answered',
      'needs_review',
      'partial',
      'blocked',
      'unknown',
      'accepted',
      'verified',
    ]) {
      final json = snapshotJson();
      (json['task'] as Map)['outcome'] = outcome;
      expect(TaskSnapshot.parse(json).task, isNotNull);
    }
    final json = snapshotJson()..['task'] = null;
    expect(TaskSnapshot.parse(json).task, isNull);
  });

  final mutations = <String, void Function(Map<String, Object?>)>{
    'unknown catalog': (j) => surfaceOf(j)['catalog_id'] = 'default',
    'unknown surface': (j) => surfaceOf(j)['surface_id'] = 'elsewhere',
    'unknown surface field': (j) => surfaceOf(j)['theme'] = {},
    'empty components': (j) => surfaceOf(j)['components'] = [],
    'missing root': (j) => componentsOf(j).first['id'] = 'other',
    'duplicate IDs': (j) => componentsOf(j)[1]['id'] = 'root',
    'unknown component': (j) => componentsOf(j)[1]['component'] = 'Text',
    'Markdown': (j) => componentsOf(j)[1]['component'] = 'Markdown',
    'image URL': (j) => componentsOf(j)[1]['component'] = 'Image',
    'openUrl': (j) => componentsOf(j)[6]['action'] = {'name': 'openUrl'},
    'functionCall': (j) => componentsOf(j)[6]['action'] = {
      'functionCall': {
        'call': 'openUrl',
        'args': ['https://example.org'],
      },
    },
    'nested event action': (j) => componentsOf(j)[6]['action'] = {
      'event': {'name': 'open_task'},
    },
    'arbitrary context': (j) => componentsOf(j)[6]['action'] = {
      'name': 'open_task',
      'context': {'command': 'echo no'},
    },
    'component URL field': (j) => componentsOf(j)[1]['url'] = 'file:///tmp/a',
    'unknown text style': (j) => componentsOf(j)[1]['style'] = 'html',
    'null optional style': (j) => componentsOf(j)[1]['style'] = null,
    'text function': (j) =>
        componentsOf(j)[3]['text'] = {'call': 'openUrl', 'args': []},
    'binding absent': (j) => componentsOf(j)[3]['text'] = {'path': '/missing'},
    'binding relative': (j) => componentsOf(j)[3]['text'] = {'path': 'context'},
    'binding nested': (j) => componentsOf(j)[3]['text'] = {'path': '/a/b'},
    'binding escaped': (j) => componentsOf(j)[3]['text'] = {'path': '/a~1b'},
    'binding extra': (j) =>
        componentsOf(j)[3]['text'] = {'path': '/context', 'default': 'bad'},
    'missing reference': (j) => componentsOf(j)[2]['child'] = 'missing',
    'self-cycle': (j) => componentsOf(j)[2]['child'] = 'card',
    'indirect cycle': (j) => componentsOf(j)[2]['child'] = 'root',
    'unreachable': (j) =>
        componentsOf(j).add({'id': 'unreachable', 'component': 'Divider'}),
    'unsafe sequence': (j) => j['sequence'] = maxSafeInteger + 1,
    'zero sequence': (j) => j['sequence'] = 0,
    'double sequence': (j) => j['sequence'] = 1.0,
    'negative task ID': (j) => (j['task'] as Map)['id'] = -1,
    'task extra field': (j) => (j['task'] as Map)['approved'] = true,
    'task unknown outcome': (j) => (j['task'] as Map)['outcome'] = 'success',
    'unsafe revision': (j) =>
        (j['task'] as Map)['work_revision'] = maxSafeInteger + 1,
    'unicode ID': (j) => componentsOf(j)[1]['id'] = '😀',
    'oversize ID': (j) => componentsOf(j)[1]['id'] = 'a' * 65,
    'empty ID': (j) => componentsOf(j)[1]['id'] = '',
    'non-string data': (j) => surfaceOf(j)['data'] = {'context': 7},
    'unicode key': (j) => surfaceOf(j)['data'] = {'é': 'no'},
    'oversize literal': (j) => componentsOf(j)[1]['text'] = 'a' * 16385,
    'oversize UTF8 data': (j) =>
        surfaceOf(j)['data'] = {'context': '😀' * 4097},
    'unpaired surrogate': (j) =>
        componentsOf(j)[1]['text'] = String.fromCharCode(0xd800),
    'oversize child list': (j) =>
        componentsOf(j).first['children'] = List.filled(65, 'title'),
    'oversize data map': (j) =>
        surfaceOf(j)['data'] = {for (var i = 0; i < 65; i++) 'key$i': ''},
    'oversize component list': (j) => surfaceOf(j)['components'] = [
      for (var i = 0; i < 257; i++) {'id': 'id$i', 'component': 'Divider'},
    ],
  };
  for (final entry in mutations.entries) {
    test('rejects ${entry.key} before any SDK mutation', () {
      final json = snapshotJson();
      entry.value(json);
      expect(() => TaskSnapshot.parse(json), invalidSnapshot);
    });
  }

  test('depth 24 allowed, depth 25 rejected', () {
    Map<String, Object?> chain(int depth) {
      final json = snapshotJson();
      surfaceOf(json)['data'] = <String, Object?>{};
      surfaceOf(json)['components'] = [
        for (var i = 0; i < depth; i++)
          {
            'id': i == 0 ? 'root' : 'n$i',
            'component': i == depth - 1 ? 'Divider' : 'Card',
            if (i != depth - 1) 'child': 'n${i + 1}',
          },
      ];
      return json;
    }

    expect(TaskSnapshot.parse(chain(24)).components.length, 24);
    expect(() => TaskSnapshot.parse(chain(25)), invalidSnapshot);
  });

  test('shared acyclic references and exponential expansion are rejected', () {
    final json = snapshotJson();
    componentsOf(json).first['children'] = [
      ...componentsOf(json).first['children'] as List,
      'title',
    ];
    expect(() => TaskSnapshot.parse(json), invalidSnapshot);
    surfaceOf(json)['data'] = <String, Object?>{};
    surfaceOf(json)['components'] = [
      for (var i = 0; i < 12; i++)
        {
          'id': i == 0 ? 'root' : 'n$i',
          'component': i == 11 ? 'Divider' : 'Column',
          if (i != 11) 'children': ['n${i + 1}', 'n${i + 1}'],
        },
    ];
    expect(() => TaskSnapshot.parse(json), invalidSnapshot);
  });

  test('unique tree admits 256 components and rejects 257', () {
    final json = snapshotJson();
    surfaceOf(json)['data'] = <String, Object?>{};
    surfaceOf(json)['components'] = [
      {
        'id': 'root',
        'component': 'Column',
        'children': ['g0', 'g1', 'g2', 'g3'],
      },
      for (var group = 0; group < 4; group++)
        {
          'id': 'g$group',
          'component': 'Column',
          'children': [
            for (
              var leaf = group * 64;
              leaf < (group == 3 ? 251 : (group + 1) * 64);
              leaf++
            )
              'leaf$leaf',
          ],
        },
      for (var leaf = 0; leaf < 251; leaf++)
        {'id': 'leaf$leaf', 'component': 'Divider'},
    ];
    expect(TaskSnapshot.parse(json).components.length, 256);
    (componentsOf(json)[4]['children'] as List).add('leaf251');
    componentsOf(
      json,
    ).add(<String, Object>{'id': 'leaf251', 'component': 'Divider'});
    expect(() => TaskSnapshot.parse(json), invalidSnapshot);
  });

  test('expanded bound text cannot multiply beyond one MiB', () {
    final json = snapshotJson();
    surfaceOf(json)['data'] = {'large': 'x' * 16384};
    surfaceOf(json)['components'] = [
      {
        'id': 'root',
        'component': 'Column',
        'children': ['a', 'b'],
      },
      {
        'id': 'a',
        'component': 'Column',
        'children': [for (var i = 0; i < 32; i++) 'leaf$i'],
      },
      {
        'id': 'b',
        'component': 'Column',
        'children': [for (var i = 32; i < 64; i++) 'leaf$i'],
      },
      for (var i = 0; i < 64; i++)
        {
          'id': 'leaf$i',
          'component': 'PlainText',
          'text': {'path': '/large'},
        },
    ];
    expect(TaskSnapshot.parse(json), isA<TaskSnapshot>());
    (componentsOf(json)[2]['children'] as List).add('leaf64');
    componentsOf(json).add(<String, Object>{
      'id': 'leaf64',
      'component': 'PlainText',
      'text': {'path': '/large'},
    });
    expect(() => TaskSnapshot.parse(json), invalidSnapshot);
  });

  test('literal URLs and Markdown remain plain inert text', () {
    final json = snapshotJson();
    componentsOf(json)[1]['text'] =
        '[go](https://evil.invalid) ![](file:///private/key) <script>';
    expect(
      TaskSnapshot.parse(json).components['title']!.text,
      isA<LiteralText>(),
    );
  });

  test('text length is UTF8 bytes, not character count', () {
    final json = snapshotJson();
    componentsOf(json)[1]['text'] = '😀' * 4096;
    expect(TaskSnapshot.parse(json), isA<TaskSnapshot>());
  });

  test('strict JSON rejects invalid, duplicate and nested payloads', () {
    for (final text in [
      '{"a":1,"a":2}',
      '{"a":1,"\\u0061":2}',
      '[1,]',
      '{"a":}',
      '{}trailing',
      'NaN',
      '{"x":"\\q"}',
      '${'[' * 34}0${']' * 34}',
    ]) {
      expect(
        () => decodeStrictJson(utf8.encode(text)),
        malformed,
        reason: text,
      );
    }
    expect(() => decodeStrictJson([0xff]), malformed);
    expect(() => decodeStrictJson([]), malformed);
    expect(
      () => decodeStrictJson(List.filled(maxFrameBytes + 1, 32)),
      malformed,
    );
    expect(decodeStrictJson(utf8.encode('{"x":"é","a":[null,true,1.5]}')), {
      'x': 'é',
      'a': [null, true, 1.5],
    });
  });

  test('envelope fields, version, direction and generation are exact', () {
    for (final kind in ['hello', 'ack', 'action', 'unknown']) {
      expect(
        () => HostMessage.parse(envelope(kind), 'test-generation'),
        malformed,
      );
    }
    expect(
      () => HostMessage.parse(
        envelope('ready')..['extra'] = 1,
        'test-generation',
      ),
      malformed,
    );
    expect(
      () => HostMessage.parse(
        envelope('ready')..['version'] = 1.0,
        'test-generation',
      ),
      malformed,
    );
    expect(
      () => HostMessage.parse(envelope('ready'), 'other'),
      throwsA(
        isA<ProtocolFailure>().having(
          (e) => e.error,
          'error',
          BridgeError.unauthorized,
        ),
      ),
    );
    for (final error in BridgeError.values) {
      expect(
        HostMessage.parse({
          ...envelope('error'),
          'code': error.code,
        }, 'test-generation').error,
        error,
      );
    }
    expect(
      () => HostMessage.parse({
        ...envelope('error'),
        'code': 'raw payload',
      }, 'test-generation'),
      malformed,
    );
  });

  test('opaque bootstrap capability uses the frame budget, not ID rules', () {
    final values = {
      'HOOTER_GENUI_SOCKET': '/Users/test/.flint/genui-runtime/g-aB123z/s',
      'HOOTER_GENUI_GENERATION': 'test-generation',
      'HOOTER_GENUI_CAPABILITY': 'opaque Δ 🔐\n${'x' * 20000}',
    };
    expect(Bootstrap.fromEnvironment(values), isA<Bootstrap>());
    expect(
      () => Bootstrap.fromEnvironment({
        ...values,
        'HOOTER_GENUI_CAPABILITY': '\n' * (maxFrameBytes ~/ 2),
      }),
      malformed,
    );
  });

  test(
    'bootstrap only permits private runtime socket and opaque env token',
    () {
      final valid = {
        'HOOTER_GENUI_SOCKET': '/Users/test/.flint/genui-runtime/g-aB123z/s',
        'HOOTER_GENUI_GENERATION': 'test-generation',
        'HOOTER_GENUI_CAPABILITY': 'test-only-not-a-real-secret',
      };
      expect(Bootstrap.fromEnvironment(valid).generation, 'test-generation');
      expect(
        Bootstrap.fromEnvironment({
          ...valid,
          'HOOTER_GENUI_SOCKET': '/Users/test/.flint/genui-runtime/g-000000/s',
        }).generation,
        'test-generation',
      );
      for (final key in valid.keys) {
        expect(
          () => Bootstrap.fromEnvironment({...valid}..remove(key)),
          throwsA(isA<ProtocolFailure>()),
        );
      }
      for (final path in [
        '/tmp/a.sock',
        'relative.sock',
        '/Users/test/.flint/genui-runtime/../a.sock',
        '/Users/test/.flint/genui-runtime/a.sock',
        '/Users/test/.flint/genui-runtime/task Ω.sock',
        '/Users/test/.flint/genui-runtime/nested/a.sock',
        '/Users/test/.flint/genui-runtime/g-short/s',
        '/Users/test/.flint/genui-runtime/g-aB123z/arbitrary.sock',
      ]) {
        expect(
          () => Bootstrap.fromEnvironment({
            ...valid,
            'HOOTER_GENUI_SOCKET': path,
          }),
          throwsA(isA<ProtocolFailure>()),
        );
      }
      expect(
        () => Bootstrap.fromEnvironment({
          ...valid,
          'HOOTER_GENUI_GENERATION': 'bad\n',
        }),
        throwsA(isA<ProtocolFailure>()),
      );
      expect(
        const ProtocolFailure(BridgeError.unauthorized).toString(),
        'Bridge failure: unauthorized',
      );
    },
  );
}
