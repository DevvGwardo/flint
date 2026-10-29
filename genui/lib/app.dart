import 'package:flutter/material.dart';

import 'bridge/client.dart';
import 'bridge/protocol.dart';
import 'rendering/snapshot_surface.dart';

ThemeData hooterTheme() => ThemeData(
  brightness: Brightness.dark,
  useMaterial3: true,
  fontFamily: '.AppleSystemUIFont',
  scaffoldBackgroundColor: const Color(0xff12171d),
  colorScheme: const ColorScheme.dark(
    primary: Color(0xffb9d9c5),
    onPrimary: Color(0xff15271c),
    secondary: Color(0xffb9d9c5),
    surface: Color(0xff1b2229),
    onSurface: Color(0xffe3e8eb),
    outline: Color(0xff53606b),
    error: Color(0xfff0b5a5),
  ),
  textTheme: const TextTheme(
    headlineMedium: TextStyle(
      fontSize: 28,
      fontWeight: FontWeight.w600,
      letterSpacing: -0.8,
    ),
    titleLarge: TextStyle(
      fontSize: 22,
      height: 1.3,
      fontWeight: FontWeight.w600,
    ),
    titleMedium: TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
    bodyLarge: TextStyle(fontSize: 15, height: 1.6),
    bodyMedium: TextStyle(fontSize: 13, height: 1.5),
    bodySmall: TextStyle(fontSize: 12, height: 1.5),
    labelSmall: TextStyle(
      fontSize: 10,
      letterSpacing: 1.5,
      fontWeight: FontWeight.w700,
    ),
  ),
  dividerColor: const Color(0xff303b44),
  filledButtonTheme: FilledButtonThemeData(
    style: FilledButton.styleFrom(
      minimumSize: const Size(48, 44),
      padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 12),
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(7)),
      textStyle: const TextStyle(fontSize: 13, fontWeight: FontWeight.w600),
    ),
  ),
);

class HooterApp extends StatefulWidget {
  const HooterApp({super.key, this.client, this.initialError});
  final BridgeClient? client;
  final BridgeError? initialError;

  @override
  State<HooterApp> createState() => _HooterAppState();
}

class _HooterAppState extends State<HooterApp> with WidgetsBindingObserver {
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.detached) widget.client?.dispose();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    widget.client?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => MaterialApp(
    debugShowCheckedModeBanner: false,
    title: 'Hooter GenUI',
    theme: hooterTheme(),
    home: TaskWindow(client: widget.client, initialError: widget.initialError),
  );
}

class TaskWindow extends StatelessWidget {
  const TaskWindow({super.key, this.client, this.initialError});
  final BridgeClient? client;
  final BridgeError? initialError;

  @override
  Widget build(BuildContext context) {
    if (client == null) return _build(context);
    return ListenableBuilder(
      listenable: client!,
      builder: (context, _) => _build(context),
    );
  }

  Widget _build(BuildContext context) {
    final snapshot = client?.snapshot;
    final phase = client?.phase ?? ConnectionPhase.failed;
    final connected = phase == ConnectionPhase.ready;
    final error = client?.error ?? initialError;
    final status = switch (phase) {
      ConnectionPhase.starting => 'Connecting',
      ConnectionPhase.authenticating => 'Authenticating',
      ConnectionPhase.ready => 'Connected to Hooter',
      ConnectionPhase.disconnected =>
        client?.connectErrno == null
            ? 'Disconnected'
            : 'Private connection unavailable (OS ${client!.connectErrno})',
      ConnectionPhase.failed => 'Connection stopped safely',
      ConnectionPhase.stopped => 'Session closed',
    };
    final theme = Theme.of(context);
    return Scaffold(
      body: SafeArea(
        child: Column(
          children: [
            Container(
              padding: const EdgeInsets.fromLTRB(24, 20, 24, 16),
              decoration: const BoxDecoration(
                color: Color(0xff171d24),
                border: Border(bottom: BorderSide(color: Color(0xff303b44))),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      const Icon(
                        Icons.shield_outlined,
                        size: 18,
                        color: Color(0xffb9d9c5),
                      ),
                      const SizedBox(width: 10),
                      Text('HOOTER', style: theme.textTheme.labelSmall),
                      const Spacer(),
                      Text(
                        'TASK VIEW',
                        style: theme.textTheme.labelSmall?.copyWith(
                          color: const Color(0xffa3adb6),
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 18),
                  Text(
                    'Work, in focus.',
                    style: theme.textTheme.headlineMedium,
                  ),
                  const SizedBox(height: 7),
                  Semantics(
                    liveRegion: true,
                    child: Row(
                      children: [
                        Icon(
                          Icons.circle,
                          size: 7,
                          color: connected
                              ? const Color(0xffb9d9c5)
                              : const Color(0xffd9be87),
                        ),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Text(status, style: theme.textTheme.bodySmall),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
            Expanded(
              child: Scrollbar(
                child: SingleChildScrollView(
                  primary: true,
                  padding: const EdgeInsets.all(24),
                  child: Align(
                    alignment: Alignment.topCenter,
                    child: ConstrainedBox(
                      constraints: const BoxConstraints(maxWidth: 820),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          _NativeTaskChrome(task: snapshot?.task),
                          const SizedBox(height: 28),
                          Text(
                            'GENERATED CONTENT',
                            style: theme.textTheme.labelSmall?.copyWith(
                              color: const Color(0xffa3adb6),
                            ),
                          ),
                          const SizedBox(height: 12),
                          if (error != null || !connected)
                            _ConnectionNotice(
                              phase: phase,
                              error: error,
                              hasSnapshot: snapshot != null,
                            ),
                          if (snapshot != null) ...[
                            if (!connected || error != null)
                              const SizedBox(height: 16),
                            ExcludeFocus(
                              excluding: !(client?.canAct ?? false),
                              child: AbsorbPointer(
                                absorbing: !(client?.canAct ?? false),
                                child: Opacity(
                                  opacity: connected ? 1 : 0.55,
                                  child: SnapshotSurface(
                                    key: ValueKey(snapshot.sequence),
                                    snapshot: snapshot,
                                    forward: client!.sendAction,
                                    onApplied: client!.applied,
                                    onFailure: () =>
                                        client!.fail(BridgeError.renderFailed),
                                  ),
                                ),
                              ),
                            ),
                          ] else if (connected)
                            const _EmptyView(
                              title: 'Ready for a task',
                              message:
                                  'The native Hooter session will send '
                                  'the next task view here.',
                            ),
                        ],
                      ),
                    ),
                  ),
                ),
              ),
            ),
            Container(
              width: double.infinity,
              padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 12),
              decoration: const BoxDecoration(
                border: Border(top: BorderSide(color: Color(0xff303b44))),
              ),
              child: Text(
                'Controls return to Hooter. Nothing runs or submits here.',
                style: theme.textTheme.bodySmall?.copyWith(
                  color: const Color(0xffa3adb6),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _NativeTaskChrome extends StatelessWidget {
  const _NativeTaskChrome({this.task});
  final HostTask? task;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final outcome = switch (task?.outcome) {
      TaskOutcome.answered => 'Answered',
      TaskOutcome.needsReview => 'Needs review',
      TaskOutcome.partial => 'Partial',
      TaskOutcome.blocked => 'Blocked',
      TaskOutcome.unknown => 'Unknown',
      TaskOutcome.accepted => 'Accepted',
      TaskOutcome.verified => 'Verified',
      null => 'No active task',
    };
    return Semantics(
      container: true,
      label: 'Trusted native task status. $outcome.',
      child: Container(
        padding: const EdgeInsets.all(18),
        decoration: BoxDecoration(
          color: const Color(0xff1e2a26),
          border: Border.all(color: const Color(0xff40564a)),
          borderRadius: BorderRadius.circular(8),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'NATIVE SESSION · HOST-OWNED STATUS',
              style: theme.textTheme.labelSmall?.copyWith(
                color: const Color(0xffb9d9c5),
              ),
            ),
            const SizedBox(height: 12),
            Wrap(
              spacing: 16,
              runSpacing: 8,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                Text(
                  task == null ? 'Waiting for Hooter' : 'Task ${task!.id}',
                  style: theme.textTheme.titleMedium,
                ),
                Container(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 10,
                    vertical: 4,
                  ),
                  decoration: BoxDecoration(
                    color: const Color(0xff2e4036),
                    borderRadius: BorderRadius.circular(4),
                  ),
                  child: Text(outcome, style: theme.textTheme.bodySmall),
                ),
              ],
            ),
            if (task != null) ...[
              const SizedBox(height: 8),
              Text(
                'Contract ${task!.contractRevision}  /  Work ${task!.workRevision}',
                style: theme.textTheme.bodySmall?.copyWith(
                  color: const Color(0xffb7c5bc),
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

class _ConnectionNotice extends StatelessWidget {
  const _ConnectionNotice({
    required this.phase,
    required this.error,
    required this.hasSnapshot,
  });
  final ConnectionPhase phase;
  final BridgeError? error;
  final bool hasSnapshot;

  @override
  Widget build(BuildContext context) {
    final starting =
        phase == ConnectionPhase.starting ||
        phase == ConnectionPhase.authenticating;
    return _EmptyView(
      title: error != null
          ? 'Task view paused safely'
          : starting
          ? 'Connecting to your native session'
          : 'The native session is no longer connected',
      message: error != null
          ? 'Hooter must relaunch this companion. Reason: ${error!.code}.'
          : starting
          ? 'Waiting for the private Hooter connection.'
          : hasSnapshot
          ? 'The last view is read-only. Reopen it from Hooter to continue.'
          : 'Open a task view from Hooter to begin.',
    );
  }
}

class _EmptyView extends StatelessWidget {
  const _EmptyView({required this.title, required this.message});
  final String title;
  final String message;
  @override
  Widget build(BuildContext context) => Container(
    padding: const EdgeInsets.symmetric(vertical: 28, horizontal: 20),
    decoration: BoxDecoration(
      border: Border.all(color: const Color(0xff303b44)),
      borderRadius: BorderRadius.circular(8),
    ),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(title, style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 8),
        Text(
          message,
          style: Theme.of(
            context,
          ).textTheme.bodyMedium?.copyWith(color: const Color(0xffa3adb6)),
        ),
      ],
    ),
  );
}
