import 'dart:convert';

const maxFrameBytes = 256 * 1024;
const maxSafeInteger = 9007199254740991;
const safeCatalogId = 'dev.hooter.safe.v1';
const taskSurfaceId = 'task';
final _identifier = RegExp(r'^[A-Za-z0-9_-]{1,64}$');
final _generation = RegExp(r'^[A-Za-z0-9-]{1,64}$');

enum BridgeError {
  malformedFrame('malformed_frame'),
  invalidSnapshot('invalid_snapshot'),
  renderFailed('render_failed'),
  unauthorized('unauthorized');

  const BridgeError(this.code);
  final String code;
}

/// Intentionally carries no payload, token, path, or parser exception.
final class ProtocolFailure implements Exception {
  const ProtocolFailure(this.error);
  final BridgeError error;
  @override
  String toString() => 'Bridge failure: ${error.code}';
}

Never _invalid() => throw const ProtocolFailure(BridgeError.invalidSnapshot);
Never _malformed() => throw const ProtocolFailure(BridgeError.malformedFrame);

bool validGeneration(Object? value) =>
    value is String && _generation.hasMatch(value);

Map<String, Object?> object(
  Object? value, {
  BridgeError error = BridgeError.invalidSnapshot,
}) {
  if (value is! Map<String, Object?>) throw ProtocolFailure(error);
  return value;
}

void exactKeys(
  Map<String, Object?> value,
  Set<String> required, [
  Set<String> optional = const {},
  BridgeError error = BridgeError.invalidSnapshot,
]) {
  if (!value.keys.toSet().containsAll(required) ||
      value.keys.any(
        (key) => !required.contains(key) && !optional.contains(key),
      )) {
    throw ProtocolFailure(error);
  }
}

int safeInteger(Object? value, {bool positive = false}) {
  if (value is! int || value < (positive ? 1 : 0) || value > maxSafeInteger) {
    _invalid();
  }
  return value;
}

String identifier(Object? value) {
  if (value is! String || !_identifier.hasMatch(value)) _invalid();
  return value;
}

String plainString(Object? value) {
  if (value is! String) _invalid();
  // Reject lone UTF-16 surrogates rather than silently replacing them.
  final units = value.codeUnits;
  for (var i = 0; i < units.length; i++) {
    final unit = units[i];
    if (unit >= 0xd800 && unit <= 0xdbff) {
      if (++i >= units.length || units[i] < 0xdc00 || units[i] > 0xdfff) {
        _invalid();
      }
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      _invalid();
    }
  }
  if (utf8.encode(value).length > 16 * 1024) _invalid();
  return value;
}

enum TaskOutcome {
  answered,
  needsReview,
  partial,
  blocked,
  unknown,
  accepted,
  verified,
}

final class HostTask {
  const HostTask(
    this.id,
    this.contractRevision,
    this.workRevision,
    this.outcome,
  );
  final int id;
  final int contractRevision;
  final int workRevision;
  final TaskOutcome outcome;

  static HostTask? parse(Object? value) {
    if (value == null) return null;
    final map = object(value);
    exactKeys(map, {'id', 'contract_revision', 'work_revision', 'outcome'});
    final outcome = switch (map['outcome']) {
      'answered' => TaskOutcome.answered,
      'needs_review' => TaskOutcome.needsReview,
      'partial' => TaskOutcome.partial,
      'blocked' => TaskOutcome.blocked,
      'unknown' => TaskOutcome.unknown,
      'accepted' => TaskOutcome.accepted,
      'verified' => TaskOutcome.verified,
      _ => _invalid(),
    };
    return HostTask(
      safeInteger(map['id']),
      safeInteger(map['contract_revision']),
      safeInteger(map['work_revision']),
      outcome,
    );
  }
}

sealed class SafeText {
  const SafeText();
  Object toJson();
  String resolve(Map<String, String> data);
}

final class LiteralText extends SafeText {
  const LiteralText(this.value);
  final String value;
  @override
  Object toJson() => value;
  @override
  String resolve(Map<String, String> data) => value;
}

final class BoundText extends SafeText {
  const BoundText(this.key);
  final String key;
  @override
  Object toJson() => <String, Object?>{'path': '/$key'};
  @override
  String resolve(Map<String, String> data) => data[key]!;
}

enum TextStyleHint { heading, body, muted, code }

enum ComponentKind { plainText, column, row, card, divider, button }

enum SafeAction { openTask, stagePrompt }

extension SafeActionName on SafeAction {
  String get wireName => switch (this) {
    SafeAction.openTask => 'open_task',
    SafeAction.stagePrompt => 'stage_prompt',
  };
}

/// All instances are constructed only after exact-shape validation.
final class SafeComponent {
  const SafeComponent._(
    this.id,
    this.kind,
    this.children, {
    this.text,
    this.style = TextStyleHint.body,
    this.action,
  });
  final String id;
  final ComponentKind kind;
  final List<String> children;
  final SafeText? text;
  final TextStyleHint style;
  final SafeAction? action;

  Map<String, Object?> toA2ui() => {
    'id': id,
    'component': switch (kind) {
      ComponentKind.plainText => 'PlainText',
      ComponentKind.column => 'Column',
      ComponentKind.row => 'Row',
      ComponentKind.card => 'Card',
      ComponentKind.divider => 'Divider',
      ComponentKind.button => 'Button',
    },
    if (text != null) 'text': text!.toJson(),
    if (text != null) 'style': style.name,
    if (kind == ComponentKind.column || kind == ComponentKind.row)
      'children': children,
    if (kind == ComponentKind.card || kind == ComponentKind.button)
      'child': children.single,
    if (action != null) 'action': {'name': action!.wireName},
  };

  static SafeComponent parse(Object? value, Map<String, String> data) {
    final map = object(value);
    final id = identifier(map['id']);
    final kind = switch (map['component']) {
      'PlainText' => ComponentKind.plainText,
      'Column' => ComponentKind.column,
      'Row' => ComponentKind.row,
      'Card' => ComponentKind.card,
      'Divider' => ComponentKind.divider,
      'Button' => ComponentKind.button,
      _ => _invalid(),
    };
    const base = {'id', 'component'};
    SafeText? text;
    var style = TextStyleHint.body;
    SafeAction? action;
    var children = <String>[];
    switch (kind) {
      case ComponentKind.plainText:
        exactKeys(map, {...base, 'text'}, {'style'});
        final value = map['text'];
        if (value is String) {
          text = LiteralText(plainString(value));
        } else {
          final binding = object(value);
          exactKeys(binding, {'path'});
          final path = binding['path'];
          if (path is! String || !path.startsWith('/')) _invalid();
          final key = identifier(path.substring(1));
          if (!data.containsKey(key)) _invalid();
          text = BoundText(key);
        }
        if (map.containsKey('style')) {
          style = switch (map['style']) {
            'heading' => TextStyleHint.heading,
            'body' => TextStyleHint.body,
            'muted' => TextStyleHint.muted,
            'code' => TextStyleHint.code,
            _ => _invalid(),
          };
        }
      case ComponentKind.column:
      case ComponentKind.row:
        exactKeys(map, {...base, 'children'});
        final refs = map['children'];
        if (refs is! List || refs.length > 64) _invalid();
        children = refs.map(identifier).toList();
      case ComponentKind.card:
        exactKeys(map, {...base, 'child'});
        children = [identifier(map['child'])];
      case ComponentKind.button:
        exactKeys(map, {...base, 'child', 'action'});
        children = [identifier(map['child'])];
        final actionMap = object(map['action']);
        exactKeys(actionMap, {'name'});
        action = switch (actionMap['name']) {
          'open_task' => SafeAction.openTask,
          'stage_prompt' => SafeAction.stagePrompt,
          _ => _invalid(),
        };
      case ComponentKind.divider:
        exactKeys(map, base);
    }
    return SafeComponent._(
      id,
      kind,
      List.unmodifiable(children),
      text: text,
      style: style,
      action: action,
    );
  }
}

final class TaskSnapshot {
  const TaskSnapshot._(this.sequence, this.task, this.components, this.data);
  final int sequence;
  final HostTask? task;
  final Map<String, SafeComponent> components;
  final Map<String, String> data;

  static TaskSnapshot parse(Map<String, Object?> envelope) {
    final sequence = safeInteger(envelope['sequence'], positive: true);
    final task = HostTask.parse(envelope['task']);
    final surface = object(envelope['surface']);
    exactKeys(surface, {'surface_id', 'catalog_id', 'components', 'data'});
    if (surface['surface_id'] != taskSurfaceId ||
        surface['catalog_id'] != safeCatalogId) {
      _invalid();
    }
    final rawData = object(surface['data']);
    if (rawData.length > 64) _invalid();
    final data = <String, String>{
      for (final entry in rawData.entries)
        identifier(entry.key): plainString(entry.value),
    };
    final rawComponents = surface['components'];
    if (rawComponents is! List ||
        rawComponents.isEmpty ||
        rawComponents.length > 256) {
      _invalid();
    }
    final components = <String, SafeComponent>{};
    for (final raw in rawComponents) {
      final component = SafeComponent.parse(raw, data);
      if (components.containsKey(component.id)) _invalid();
      components[component.id] = component;
    }
    if (!components.containsKey('root')) _invalid();
    final parents = <String>{};
    for (final component in components.values) {
      for (final child in component.children) {
        if (child == 'root' || !parents.add(child)) _invalid();
      }
    }
    final active = <String>{};
    final reached = <String>{};
    var expanded = 0;
    var textBytes = 0;
    // A strict tree matches the native boundary and prevents shared-child
    // expansion before the SDK's recursive widget builder runs.
    void walk(String id, int depth) {
      if (depth > 24 || ++expanded > 1024 || !active.add(id)) _invalid();
      final component = components[id];
      if (component == null) _invalid();
      reached.add(id);
      if (component.text != null) {
        textBytes += utf8.encode(component.text!.resolve(data)).length;
        if (textBytes > 1024 * 1024) _invalid();
      }
      for (final child in component.children) {
        walk(child, depth + 1);
      }
      active.remove(id);
    }

    walk('root', 1);
    if (reached.length != components.length) _invalid();
    return TaskSnapshot._(
      sequence,
      task,
      Map.unmodifiable(components),
      Map.unmodifiable(data),
    );
  }
}

enum HostMessageKind { ready, snapshot, error, shutdown }

final class HostMessage {
  const HostMessage._(this.kind, {this.snapshot, this.error});
  final HostMessageKind kind;
  final TaskSnapshot? snapshot;
  final BridgeError? error;

  static HostMessage parse(Object? value, String generation) {
    final map = object(value, error: BridgeError.malformedFrame);
    if (map['version'] is! int || map['version'] != 1) _malformed();
    if (!validGeneration(map['generation']) ||
        map['generation'] != generation) {
      throw const ProtocolFailure(BridgeError.unauthorized);
    }
    const base = {'version', 'kind', 'generation'};
    switch (map['kind']) {
      case 'ready':
        exactKeys(map, base, const {}, BridgeError.malformedFrame);
        return const HostMessage._(HostMessageKind.ready);
      case 'shutdown':
        exactKeys(map, base, const {}, BridgeError.malformedFrame);
        return const HostMessage._(HostMessageKind.shutdown);
      case 'error':
        exactKeys(map, {...base, 'code'}, const {}, BridgeError.malformedFrame);
        final error = BridgeError.values.where((e) => e.code == map['code']);
        if (error.isEmpty) _malformed();
        return HostMessage._(HostMessageKind.error, error: error.single);
      case 'snapshot':
        exactKeys(
          map,
          {...base, 'sequence', 'task', 'surface'},
          const {},
          BridgeError.invalidSnapshot,
        );
        return HostMessage._(
          HostMessageKind.snapshot,
          snapshot: TaskSnapshot.parse(map),
        );
      default:
        _malformed();
    }
  }
}

/// Checks nesting and duplicate object keys *before* Dart's JSON decoder,
/// which otherwise drops duplicate keys and accepts deeply nested input.
Object? decodeStrictJson(List<int> bytes) {
  if (bytes.isEmpty || bytes.length > maxFrameBytes) _malformed();
  try {
    final text = utf8.decode(bytes, allowMalformed: false);
    _JsonGuard(text).check();
    return jsonDecode(text);
  } on ProtocolFailure {
    rethrow;
  } catch (_) {
    _malformed();
  }
}

final class _JsonGuard {
  _JsonGuard(this.text);
  final String text;
  var offset = 0;
  var nodes = 0;

  void check() {
    _value(0);
    _space();
    if (offset != text.length) _malformed();
  }

  void _space() {
    while (offset < text.length &&
        const [9, 10, 13, 32].contains(text.codeUnitAt(offset))) {
      offset++;
    }
  }

  void _value(int depth) {
    if (depth > 32 || ++nodes > 16384) _malformed();
    _space();
    if (offset >= text.length) _malformed();
    final char = text[offset];
    if (char == '{') {
      offset++;
      final keys = <String>{};
      _space();
      if (_take('}')) return;
      do {
        _space();
        final key = _string();
        if (!keys.add(key)) _malformed();
        _space();
        if (!_take(':')) _malformed();
        _value(depth + 1);
        _space();
        if (_take('}')) return;
        if (!_take(',')) _malformed();
      } while (true);
    } else if (char == '[') {
      offset++;
      _space();
      if (_take(']')) return;
      do {
        _value(depth + 1);
        _space();
        if (_take(']')) return;
        if (!_take(',')) _malformed();
      } while (true);
    } else if (char == '"') {
      _string();
    } else {
      final start = offset;
      while (offset < text.length &&
          !const [
            ',',
            ']',
            '}',
            ' ',
            '\n',
            '\r',
            '\t',
          ].contains(text[offset])) {
        offset++;
      }
      if (offset == start) _malformed();
      // jsonDecode performs strict number/literal validation afterwards.
    }
  }

  bool _take(String char) {
    if (offset < text.length && text[offset] == char) {
      offset++;
      return true;
    }
    return false;
  }

  String _string() {
    final start = offset;
    if (!_take('"')) _malformed();
    while (offset < text.length) {
      final char = text[offset++];
      if (char == '\\') {
        if (offset >= text.length) _malformed();
        offset++;
      } else if (char == '"') {
        final value = jsonDecode(text.substring(start, offset));
        if (value is! String) _malformed();
        return value;
      }
    }
    _malformed();
  }
}
