import 'dart:async';
import 'dart:ui' show SemanticsAction, Tristate;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:genui/genui.dart' as genui;
import 'package:hooter_genui/app.dart';
import 'package:hooter_genui/bridge/protocol.dart';
import 'package:hooter_genui/rendering/safe_catalog.dart';
import 'package:hooter_genui/rendering/snapshot_surface.dart';
import 'package:logging/logging.dart';

import 'fixtures.dart';

void main() {
  setUp(() => genui.configureLogging(level: Level.OFF, logCallback: (_, _) {}));

  Widget harness(
    TaskSnapshot snapshot, {
    ForwardAction? forward,
    void Function(TaskSnapshot)? applied,
    void Function(PreparedSurface)? prepared,
    ThemeData? theme,
  }) => MaterialApp(
    theme: theme ?? hooterTheme(),
    home: Scaffold(
      body: SingleChildScrollView(
        padding: const EdgeInsets.all(20),
        child: SnapshotSurface(
          key: ValueKey(snapshot.sequence),
          snapshot: snapshot,
          forward: forward ?? (_, _, _) => true,
          onApplied: applied ?? (_) {},
          onFailure: () => fail('SDK render/validation failure'),
          onPrepared: prepared,
        ),
      ),
    ),
  );

  testWidgets('genuine SDK renders six custom component types and bindings', (
    tester,
  ) async {
    PreparedSurface? prepared;
    final acks = <int>[];
    await tester.pumpWidget(
      harness(
        fixture(),
        prepared: (p) => prepared = p,
        applied: (s) => acks.add(s.sequence),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.byType(genui.Surface), findsOneWidget);
    expect(find.byType(genui.BoundString), findsNWidgets(4));
    expect(find.text('Bound task context'), findsOneWidget);
    expect(find.text('A task worth reviewing'), findsOneWidget);
    expect(find.byType(FilledButton), findsNWidgets(2));
    expect(find.byType(Divider), findsOneWidget);
    expect(prepared!.controller.contextFor('task').catalog!.functions, isEmpty);
    expect(acks, [1]);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
    expect(prepared!.disposed, true);
  });

  testWidgets('native button labels meet readable contrast for all styles', (
    tester,
  ) async {
    await tester.pumpWidget(
      harness(
        fixture(),
        theme: hooterTheme().copyWith(visualDensity: VisualDensity.compact),
      ),
    );
    await tester.pumpAndSettle();
    final button = tester.widget<FilledButton>(find.byType(FilledButton).first);
    final background = button.style!.backgroundColor!.resolve({})!;
    final label = tester.widget<Text>(find.text('Open native task'));
    for (final foreground in [
      label.style!.color!,
      hooterTheme().textTheme.titleLarge!.color!,
      const Color(0xffa3adb6),
      const Color(0xffc9d4d9),
    ]) {
      final a = background.computeLuminance();
      final b = foreground.computeLuminance();
      final contrast = ((a > b ? a : b) + 0.05) / ((a < b ? a : b) + 0.05);
      expect(contrast, greaterThanOrEqualTo(4.5));
    }
    expect(
      button.style!.minimumSize!.resolve({})!.height,
      greaterThanOrEqualTo(44),
    );
    final material = find
        .descendant(
          of: find.byType(FilledButton).first,
          matching: find.byType(Material),
        )
        .first;
    expect(tester.getSize(material).height, greaterThanOrEqualTo(44));
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });

  testWidgets('SDK semantics label buttons and forward only safe actions', (
    tester,
  ) async {
    final semantics = tester.ensureSemantics();
    try {
      final forwarded = <List<Object>>[];
      await tester.pumpWidget(
        harness(
          fixture(),
          forward: (sequence, source, name) {
            forwarded.add([sequence, source, name]);
            return true;
          },
        ),
      );
      await tester.pumpAndSettle();
      for (final label in ['Open native task', 'Prepare follow-up']) {
        final node = tester.getSemantics(
          find.widgetWithText(FilledButton, label),
        );
        final data = node.getSemanticsData();
        expect(data.label, label);
        expect(data.flagsCollection.isButton, true);
        expect(data.flagsCollection.isEnabled, Tristate.isTrue);
        expect(data.hasAction(SemanticsAction.tap), true);
        node.owner!.performAction(node.id, SemanticsAction.tap);
        await tester.pumpAndSettle();
      }
      expect(forwarded, [
        [1, 'open', 'open_task'],
        [1, 'stage', 'stage_prompt'],
      ]);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    } finally {
      semantics.dispose();
    }
  });

  testWidgets('Button uses explicit delegate; SDK receives no submission', (
    tester,
  ) async {
    PreparedSurface? prepared;
    final forwarded = <List<Object>>[];
    final sdkEvents = <genui.ChatMessage>[];
    await tester.pumpWidget(
      harness(
        fixture(),
        prepared: (p) => prepared = p,
        forward: (sequence, source, name) {
          forwarded.add([sequence, source, name]);
          return true;
        },
      ),
    );
    await tester.pumpAndSettle();
    final subscription = prepared!.controller.onSubmit.listen(sdkEvents.add);
    await tester.tap(find.text('Open native task'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Prepare follow-up'));
    await tester.pumpAndSettle();
    expect(forwarded, [
      [1, 'open', 'open_task'],
      [1, 'stage', 'stage_prompt'],
    ]);
    expect(sdkEvents, isEmpty);
    // Cancellation detaches synchronously. Awaiting Dart's shared completed
    // cancellation future deadlocks this toolchain's fake-async test zone.
    unawaited(subscription.cancel());
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });

  testWidgets('delegate consumes malformed/unknown/function/context events', (
    tester,
  ) async {
    PreparedSurface? prepared;
    var count = 0;
    await tester.pumpWidget(harness(fixture(), prepared: (p) => prepared = p));
    await tester.pumpAndSettle();
    final delegate = SafeActionDelegate(fixture(), (_, _, _) {
      count++;
      return true;
    });
    final context = tester.element(find.byType(genui.Surface));
    for (final event in <Map<String, Object?>>[
      {},
      {'name': 'open_task', 'sourceComponentId': 'title'},
      {'name': 'openUrl', 'sourceComponentId': 'open'},
      {'name': 'stage_prompt', 'sourceComponentId': 'open'},
      {
        'name': 'open_task',
        'sourceComponentId': 'open',
        'context': {'command': 'do not run'},
      },
      {
        'name': 'open_task',
        'sourceComponentId': 'open',
        'functionCall': {'call': 'openUrl'},
      },
      {'name': 'open_task', 'sourceComponentId': 'open', 'surfaceId': 'wrong'},
      {'name': 'open_task', 'sourceComponentId': 'open', 'unexpected': true},
    ]) {
      expect(
        delegate.handleEvent(
          context,
          genui.UiEvent.fromMap(event),
          prepared!.controller.contextFor('task'),
          (_, _, _, _) => const SizedBox.shrink(),
        ),
        true,
      );
    }
    expect(count, 0);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });

  testWidgets('replacement is full, old controller disposes after unmount', (
    tester,
  ) async {
    final prepared = <PreparedSurface>[];
    await tester.pumpWidget(harness(fixture(), prepared: prepared.add));
    await tester.pumpAndSettle();
    final json = snapshotJson(sequence: 2);
    surfaceOf(json)['components'] = [
      {
        'id': 'root',
        'component': 'PlainText',
        'text': {'path': '/fresh'},
      },
    ];
    surfaceOf(json)['data'] = {'fresh': 'Only new state'};
    await tester.pumpWidget(
      harness(TaskSnapshot.parse(json), prepared: prepared.add),
    );
    await tester.pumpAndSettle();
    expect(prepared.length, 2);
    expect(prepared.first.disposed, true);
    expect(prepared.last.disposed, false);
    expect(find.text('Only new state'), findsOneWidget);
    expect(find.text('Bound task context'), findsNothing);
    expect(find.byType(FilledButton), findsNothing);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
    expect(prepared.last.disposed, true);
    expect(tester.takeException(), isNull);
  });

  for (final contextError in [false, true]) {
    testWidgets('${contextError ? 'Synchronous context' : 'Queued controller'} '
        'SDK error is sanitized and never acknowledged', (tester) async {
      final acknowledgements = <int>[];
      var failures = 0;
      PreparedSurface? prepared;
      await tester.pumpWidget(
        MaterialApp(
          theme: hooterTheme(),
          home: Scaffold(
            body: SnapshotSurface(
              snapshot: fixture(),
              forward: (_, _, _) => false,
              onApplied: (s) => acknowledgements.add(s.sequence),
              onFailure: () => failures++,
              onPrepared: (p) {
                prepared = p;
                final error = StateError(
                  'sensitive payload must not be displayed',
                );
                if (contextError) {
                  p.context.reportError(error, StackTrace.current);
                } else {
                  p.controller.reportError(error, StackTrace.current);
                }
              },
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(acknowledgements, isEmpty);
      expect(failures, 1);
      expect(
        find.text('This task view could not be rendered safely.'),
        findsOneWidget,
      );
      expect(find.textContaining('sensitive payload'), findsNothing);
      expect(prepared!.failed, true);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
      expect(prepared!.disposed, true);
    });
  }

  testWidgets('URLs, Markdown and HTML display literally, with no assets', (
    tester,
  ) async {
    const text =
        '[link](https://evil.invalid) ![](file:///private/key) <b>raw</b>';
    await tester.pumpWidget(harness(fixture(text: text)));
    await tester.pumpAndSettle();
    expect(find.text(text), findsOneWidget);
    expect(find.byType(Image), findsNothing);
    expect(find.byType(RichText), findsWidgets); // Native Text internals.
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });

  testWidgets('narrow layout, long text, scrolling and keyboard activation', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(420, 700);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    var actions = 0;
    await tester.pumpWidget(
      harness(
        fixture(text: 'word ' * 300),
        forward: (_, _, _) {
          actions++;
          return true;
        },
      ),
    );
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
    await tester.ensureVisible(find.text('Open native task'));
    await tester.pumpAndSettle();
    await tester.sendKeyEvent(LogicalKeyboardKey.tab);
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();
    expect(actions, 1);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });

  testWidgets('missing bootstrap has clear sanitized trusted chrome', (
    tester,
  ) async {
    await tester.pumpWidget(
      const HooterApp(initialError: BridgeError.unauthorized),
    );
    await tester.pumpAndSettle();
    expect(find.text('NATIVE SESSION · HOST-OWNED STATUS'), findsOneWidget);
    expect(find.text('No active task'), findsOneWidget);
    expect(find.text('Task view paused safely'), findsOneWidget);
    expect(find.textContaining('unauthorized'), findsOneWidget);
    expect(find.byType(genui.Surface), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
