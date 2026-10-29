import 'package:flutter/material.dart';
import 'package:genui/genui.dart' as genui;
import 'package:json_schema_builder/json_schema_builder.dart';

import '../bridge/protocol.dart';

/// No default catalog, client functions, media, Markdown or URL launchers.
/// Schemas mirror our ingress contract; SDK schemas are not a security gate.
genui.Catalog createSafeCatalog() {
  final reference = S.string();
  final children = S.list(items: reference);
  return genui.Catalog([
    genui.CatalogItem(
      name: 'PlainText',
      dataSchema: S.object(
        properties: {
          'text': Schema.fromMap({
            'anyOf': [
              {'type': 'string'},
              {
                'type': 'object',
                'properties': {
                  'path': {'type': 'string'},
                },
                'required': ['path'],
                'additionalProperties': false,
              },
            ],
          }),
          'style': S.string(enumValues: ['heading', 'body', 'muted', 'code']),
        },
        required: ['text'],
      ),
      widgetBuilder: (item) {
        final data = item.data as Map<String, Object?>;
        final style = data['style'] as String? ?? 'body';
        // Genuine SDK data binding, with native Flutter Text, not Markdown.
        return genui.BoundString(
          dataContext: item.dataContext,
          value: data['text']!,
          builder: (context, value) {
            final theme = Theme.of(context);
            return Text(
              value ?? '',
              softWrap: true,
              style: switch (style) {
                'heading' => theme.textTheme.titleLarge,
                'muted' => theme.textTheme.bodyMedium?.copyWith(
                  color: const Color(0xffa3adb6),
                ),
                'code' => theme.textTheme.bodyMedium?.copyWith(
                  fontFamily: 'Menlo',
                  color: const Color(0xffc9d4d9),
                ),
                _ => theme.textTheme.bodyLarge,
              },
            );
          },
        );
      },
    ),
    for (final type in ['Column', 'Row'])
      genui.CatalogItem(
        name: type,
        dataSchema: S.object(
          properties: {'children': children},
          required: ['children'],
        ),
        widgetBuilder: (item) {
          final data = item.data as Map<String, Object?>;
          final ids = (data['children'] as List).cast<String>();
          final widgets = ids.map(item.buildChild).toList();
          if (type == 'Column') {
            return Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              spacing: 16,
              children: widgets,
            );
          }
          // Each child gets bounded width and may wrap long native Text.
          // Below 480px, stack instead of overflowing a narrow window.
          return LayoutBuilder(
            builder: (context, constraints) {
              if (constraints.maxWidth < 480 ||
                  (ids.isNotEmpty && constraints.maxWidth / ids.length < 140)) {
                return Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  spacing: 12,
                  children: widgets,
                );
              }
              return Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                spacing: 12,
                children: [for (final child in widgets) Expanded(child: child)],
              );
            },
          );
        },
      ),
    genui.CatalogItem(
      name: 'Card',
      dataSchema: S.object(
        properties: {'child': reference},
        required: ['child'],
      ),
      widgetBuilder: (item) {
        final data = item.data as Map<String, Object?>;
        return Container(
          width: double.infinity,
          padding: const EdgeInsets.all(20),
          decoration: BoxDecoration(
            color: const Color(0xff1b2229),
            border: Border.all(color: const Color(0xff303b44)),
            borderRadius: BorderRadius.circular(10),
          ),
          child: item.buildChild(data['child'] as String),
        );
      },
    ),
    genui.CatalogItem(
      name: 'Divider',
      dataSchema: S.object(properties: {}),
      widgetBuilder: (_) => const Divider(height: 1),
    ),
    genui.CatalogItem(
      name: 'Button',
      dataSchema: S.object(
        properties: {
          'child': reference,
          'action': S.object(
            properties: {
              'name': S.string(enumValues: ['open_task', 'stage_prompt']),
            },
            required: ['name'],
          ),
        },
        required: ['child', 'action'],
      ),
      widgetBuilder: (item) {
        final data = item.data as Map<String, Object?>;
        final action = data['action'] as Map<String, Object?>;
        // Deliberately not the upstream Button (its functionCall path
        // bypasses ActionDelegate). Only dispatch an inert SDK UI event.
        return FilledButton.tonal(
          // PlainText intentionally uses explicit, readable catalog styles.
          // Keep every allowed label style readable against the button too.
          style: FilledButton.styleFrom(
            backgroundColor: const Color(0xff243e35),
            foregroundColor: const Color(0xffe6efeb),
            minimumSize: const Size(0, 44),
            visualDensity: VisualDensity.standard,
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
            side: const BorderSide(color: Color(0xff446153)),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(8),
            ),
          ),
          onPressed: () => item.dispatchEvent(
            genui.UserActionEvent(
              name: action['name'] as String,
              sourceComponentId: item.id,
              context: const {},
            ),
          ),
          child: item.buildChild(data['child'] as String),
        );
      },
    ),
  ], catalogId: safeCatalogId);
}

typedef ForwardAction = bool Function(int sequence, String source, String name);

/// Always returns true, including for malformed and unknown SDK events.
/// Nothing reaches the SDK's default submission or function behavior.
final class SafeActionDelegate implements genui.ActionDelegate {
  const SafeActionDelegate(this.snapshot, this.forward);
  final TaskSnapshot snapshot;
  final ForwardAction forward;

  @override
  bool handleEvent(
    BuildContext context,
    genui.UiEvent event,
    genui.SurfaceContext genUiContext,
    Widget Function(
      genui.SurfaceDefinition,
      genui.Catalog,
      String,
      genui.DataContext,
    )
    buildWidget,
  ) {
    final data = event.toMap();
    final source = data['sourceComponentId'];
    final name = data['name'];
    final component = snapshot.components[source];
    final eventContext = data['context'];
    const allowed = {
      'name',
      'sourceComponentId',
      'timestamp',
      'context',
      'surfaceId',
    };
    if (genUiContext.surfaceId == taskSurfaceId &&
        data.keys.every(allowed.contains) &&
        source is String &&
        name is String &&
        component?.kind == ComponentKind.button &&
        component?.action?.wireName == name &&
        (eventContext == null ||
            (eventContext is Map && eventContext.isEmpty)) &&
        !data.containsKey('functionCall') &&
        (data['surfaceId'] == null || data['surfaceId'] == taskSurfaceId)) {
      forward(snapshot.sequence, source, name);
    }
    return true;
  }
}
