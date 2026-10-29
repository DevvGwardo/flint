import 'package:hooter_genui/bridge/protocol.dart';

Map<String, Object?> envelope(String kind) => {
  'version': 1,
  'kind': kind,
  'generation': 'test-generation',
};

Map<String, Object?> snapshotJson({
  int sequence = 1,
  String text = 'Bound task context',
}) => {
  ...envelope('snapshot'),
  'sequence': sequence,
  'task': {
    'id': 7,
    'contract_revision': 3,
    'work_revision': 4,
    'outcome': 'needs_review',
  },
  'surface': {
    'surface_id': taskSurfaceId,
    'catalog_id': safeCatalogId,
    'data': {'context': text},
    'components': <Map<String, Object?>>[
      <String, Object?>{
        'id': 'root',
        'component': 'Column',
        'children': ['title', 'card', 'divider', 'actions'],
      },
      <String, Object?>{
        'id': 'title',
        'component': 'PlainText',
        'text': 'A task worth reviewing',
        'style': 'heading',
      },
      <String, Object?>{'id': 'card', 'component': 'Card', 'child': 'context'},
      <String, Object?>{
        'id': 'context',
        'component': 'PlainText',
        'text': {'path': '/context'},
      },
      <String, Object?>{'id': 'divider', 'component': 'Divider'},
      <String, Object?>{
        'id': 'actions',
        'component': 'Row',
        'children': ['open', 'stage'],
      },
      <String, Object?>{
        'id': 'open',
        'component': 'Button',
        'child': 'open_label',
        'action': {'name': 'open_task'},
      },
      <String, Object?>{
        'id': 'open_label',
        'component': 'PlainText',
        'text': 'Open native task',
      },
      <String, Object?>{
        'id': 'stage',
        'component': 'Button',
        'child': 'stage_label',
        'action': {'name': 'stage_prompt'},
      },
      <String, Object?>{
        'id': 'stage_label',
        'component': 'PlainText',
        'text': 'Prepare follow-up',
      },
    ],
  },
};

TaskSnapshot fixture({int sequence = 1, String text = 'Bound task context'}) =>
    HostMessage.parse(
      snapshotJson(sequence: sequence, text: text),
      'test-generation',
    ).snapshot!;

Map<String, Object?> surfaceOf(Map<String, Object?> snapshot) =>
    snapshot['surface'] as Map<String, Object?>;
List<Map<String, Object?>> componentsOf(Map<String, Object?> snapshot) =>
    (surfaceOf(snapshot)['components'] as List).cast<Map<String, Object?>>();
