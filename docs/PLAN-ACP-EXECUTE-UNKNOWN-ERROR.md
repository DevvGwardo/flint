# Plan: Droid commands fail with "Error executing command: Unknown error"

## Implementation checkpoint, 2026-10-03

The source now contains terminal diagnostics, conservative shell-line
compatibility, multiline command/approval rendering fixes, and agent/model/
provider retention. Offline validation evidence lives in
`target/acp-execute-selection-20261003/`; `verify.py` reproduces formatting,
strict Clippy, workspace tests, serial UI tests and the release build with an
isolated home and live inference disabled. Initial scoped source copies and
the previous release binary are preserved there.

Foreground execution reproduced the original error during this task.
Background execution worked and was used for finite validation commands.
The original failing live `terminal/create` request was not captured, so cause
B remains a hypothesis about those sessions, not a confirmed live diagnosis.
The compatibility fix is independently demonstrated by local regressions.

No running app was restarted, no provider-backed prompt was sent, no real
credentials were inspected, and no external push was performed. A regression
pushes only a synthetic blob tag to a disposable local Git remote.

## Native follow-up, 2026-10-03

The rebuilt binary has now run foreground echo, npm tests, successful and
failing heredocs, and a missing executable through an offline ACP peer in a
disposable symlinked workspace. Native screenshots show separate, readable
command rows, the real missing-binary reason, and the full expanded heredoc.
Native model/provider picker actions, new session and `/clear` delivered the
selected identity to the peer before its first prompt. An isolated restart
into another workspace also preserved the pair on the wire.

Review found two additional multiline display paths: the running-task tray
and mirrored terminal-tab captions. Both had 88-pixel summaries in fixed-line
layouts and now have failing-before/passing-after regressions. Summaries and
session activity stay on one line; raw commands and exit status remain intact.
Work disclosures, command rows and option-menu rows also expose named native
accessibility buttons, allowing background verification without blind clicks.

The reproducible native harness is `tools/blueprint/acp.py`, with
`acp_fixture.py` as the offline peer. Intermediate evidence is retained under
`target/acp-native-20261003-04/` and `target/acp-native-20261003-05/`.
These are native integration checks, not real Droid inference. The first
complete native workflow captured a restart screenshot without restored controls;
its protocol checks passed but its UI assertion did not. Final results are recorded
below. The last background-only run stops on unconfirmed keyboard delivery.
Foreground escalation is deliberately not automatic.

Final offline results:

- Full workspace checkpoint: ACP 68, agent 150, app 122, UI 212 and terminal 17,
  with formatting, strict Clippy, separate serial UI and release build passing.
  Evidence: `target/acp-closeout-20261003/`.
- Final terminal-tab refinement: app 122 and UI 213 pass, including both new
  multiline regressions. Formatting, strict workspace/all-target Clippy,
  release build and whitespace checks pass. Source hashes are unchanged.
  Evidence: `target/acp-closeout-final-20261003/`.
- Native foreground execution, real error display, work/command expansion,
  picker changes and selection restoration before new-session/clear prompts
  pass. Compact approval and isolated restart/provider restoration on the wire
  passed in the `-05` run. The final native rerun is **not fully passed**:
  background text delivery after `/clear` was unconfirmed, with Cua explicitly
  recommending foreground escalation.
  Evidence: `target/acp-native-final-20261003-02/`.

To finish the remaining native workflow, permission is needed for brief
foreground control of the disposable test window. Restarting the user's active
app and authenticating real Droid tests need separate explicit authorization.

The actual app hosting this conversation has not been restarted. Authenticated
Droid smoke tests in `~/Website` and `~/yeti-trader` require explicit permission
to override the repository's no-provider/no-real-credentials safeguards.
The historical failing request was not captured, so cause B is still unconfirmed.

## Symptom

In flint sessions driven by Droid over ACP, the `execute` tool fails almost
immediately (2–100 ms) with:

```
Error: Error executing command: Unknown error
```

The command never runs. Droid then tells the user that "shell execution is
failing" and gives up on tests, builds and `git push`.

Seen in:

| Session | Workspace | Failures |
|---|---|---|
| `18db28b6f11c70e0` "Fix the failing tests" | `/Users/devgwardo/yeti-trader` (symlink → `/Volumes/T7 Shield/mac-offload/Projects/yeti-trader`) | `npm test`, `tsc` |
| `18db23358f245d20` "Add input validation" | `/Users/devgwardo/Website` (real directory) | 20+ commands, including multi-line `python3 - <<'PY'` |

Droid's own background runner worked in the same session ("Background process
started (PID …)"), so the failure is in the foreground path, which goes
through flint's ACP terminal (`terminal/create`).

## What we know

1. flint answers `terminal/create` in `crates/flint-acp/src/terminals.rs`
   (`Terminals::create`). Any `Err` becomes a JSON-RPC `invalid_params`
   error (`files.rs::rpc_error`).
2. The old client sent its reason already, but Droid displayed "Unknown error".
   The client now logs redacted request shape and the reason, and independently
   forwards diagnostics to the transcript so the adapter cannot erase them.
3. **Cause A (confirmed and fixed):** `inside_workspace` compared the
   requested `cwd` against the workspace as text. When the workspace is a
   symlink, Droid sends the resolved real path, which was rejected as
   "outside the workspace". Fixed in `crates/flint-acp/src/files.rs`, with the
   test `symlinked_workspace_accepts_its_real_path`. Rebuilding does not replace
   the executable image of an already-running app; restart is a separate step.
4. Cause A does **not** explain the `Website` session, because that
   workspace isn't a symlink. A second cause exists.

## Suspected cause B: whole shell line passed as `command`

The old `Terminals::create` ran `Command::new(&request.command).args(&request.args)`
without a shell. If Droid sends the full shell line as `command` with empty
`args` (for example `cd "…" && npm test` or a heredoc), the spawn fails with
`ENOENT`. Every command fails, however simple, and it fails fast. That
matches what we see.

The ACP schema describes a command plus optional arguments, but does not
mandate interpreting every command as shell text. The compatibility path
therefore preserves executable/argv semantics first.

## Steps

### 1. Make the failure visible (do first)
- [x] In `runner.rs` (`CreateTerminalRequest` handler), log failure warnings:
      command byte count, multiline/shell-syntax flags, argument count, cwd,
      environment names, and the redacted reason. Never log command, argument
      or environment values, including their escaped forms.
- [x] Put flint's reason into the error flint sends back. Also show it in
      flint's UI on the tool row, so "Unknown error" is never all the user
      sees.
- [ ] After restarting, run one Droid command in `~/Website` and record the
      redacted `terminal/create` request shape and reason. This confirms or
      rules out cause B for the live session without logging sensitive scripts.

### 2. Support shell-line adapters without breaking argv
- [x] Try the literal executable first. Only after not-found/name-too-long,
      with no explicit arguments and shell syntax, use `$SHELL -c <command>`.
      Existing executable paths containing spaces/metacharacters remain direct;
      missing literal paths are not blindly interpreted. Invalid configured
      shells fall back to `/bin/sh`.
- [x] Use the adapter's GUI-safe default PATH for terminals, preserving an
      explicit request PATH override. Validate cwd before either spawn.
- [x] Make the spawn error say `cannot run <program>: <err> (cwd: <cwd>)`.
- [x] Tests in `terminals.rs`:
  - `command = "echo a && echo b"`, `args = []` → output `a\nb`, exit 0
  - multiline `cat` heredoc and scripts longer than a filename run
  - explicit arguments containing metacharacters stay literal
  - a missing binary returns a clear error, not an empty one
  - literal executable paths with spaces/metacharacters stay direct
  - environment overrides and invalid-shell fallback work
  - missing cwd produces no terminal-start event; diagnostic values stay private

### 3. Ship cause A
- [ ] Restart Flint after the validated release build (not performed automatically).
- [ ] Check in a symlinked workspace (`~/yeti-trader`) that `npm test` runs.

### 4. Keep the model from giving up
- [x] When a `terminal/create` fails, append a short note to the tool
      result: the reason and "this is a flint terminal error, not a broken
      shell". Preserve it even if the adapter later reports only "Unknown
      error". Correlate only a unique exact command/argv match; otherwise use
      a separate failed diagnostic row instead of guessing a concurrent call.
      The RPC response also includes the note, but a Droid adapter that
      discards RPC error messages may still hide it from its model. The UI
      diagnostic does not claim to alter the ACP agent's conversation.

### 5. UI: multi-line commands overlap neighbouring rows

This affects **every** multi-line command row, failed (red) or successful
(✓). The `python3 - <<'PY' / import os,re / from pathlib import Path / root=…`
text draws over the row's own sub-line ("└ 1 line", "Note: Process will
continue…") and over the rows above and below.

**Cause:** in `crates/flint-app/src/transcript/rows.rs` (the tool row,
around line 377), the header is a fixed `.h(px(26.))` flex row. The target
cell uses `.truncate()`, which only clips *horizontally*. `call.summary`
comes straight from the ACP tool title (`flint-acp/src/mapper.rs:192`,
`summary: title.clone()`), and Droid puts the full command there, newlines
included. GPUI lays out every line, and the extra lines overflow the 26px
row.

Fix:
- [x] Add a helper, `one_line(summary) -> String`: first non-empty line,
      trimmed, plus " …" when more lines follow. Use it for `target` in
      `rows.rs`, and for the approval prompt summary, which has the same
      issue.
- [x] Also clip the header vertically (`.overflow_hidden()` on the 26px row)
      as a safety net for any text that slips through.
- [x] Show the full multi-line command only in the expanded view, as a
      proper mono block that wraps.
      Full titles and raw arguments are preserved; optional mapper
      normalisation was deliberately omitted to avoid losing request details.
      Explicit argv is shown with shell-safe quoting, so expanded details and
      terminal actions do not drop arguments or reinterpret literal metacharacters.
- [x] Tests: a unit test for `one_line` (single line, heredoc, leading blank
      lines, CRLF), and `blueprint_ui` layout assertions with a multiline command
      row between two ordinary rows, both failed and successful, checking that
      row bounds don't overlap.
      Approval-summary bounds and expansion of the full command are also
      checked. These are headless layout checks, not pixel screenshots.

### 6. Droid's background workaround leaves processes behind

Once foreground commands fail, Droid starts running them as **background
processes** ("Note: Process will continue after CLI exits. Use 'ps' or
'kill' commands to manage.") and reads their output from
`/var/folders/.../T/droid-bg-*.out`. That works, but each command leaves a
process flint doesn't own or clean up, and the output doesn't stream into
flint's terminal tabs.
- [ ] Verify that Droid goes back to
      foreground `execute` in a new session.
- [x] Document rollout guidance: start new Droid sessions after restarting the fixed app, rather than
      resuming sessions that learned the workaround. This does not clean up
      processes left by the old workaround. No unrelated processes were killed.

### 7. Retain the selected agent, model and provider

**Confirmed source causes:** `Session::new` defaults to Flint, and new-session/
clear paths did not inherit the active agent. ACP selections in `acp.json`
belonged only to the existing conversation, so a genuinely new one asked the
adapter for its defaults.

- [x] New sessions and `/clear` inherit the active agent. Empty-session reuse
      requires the same agent and workspace. Explicit agent changes never
      start the previous adapter in the new session.
- [x] Persist the selected agent for the fresh session on app startup.
- [x] New-session, clear and project-switch paths seed missing identity
      preferences from an older conversation's confirmed options. They never
      overwrite a newer saved default from another conversation.
- [x] Persist confirmed model/provider choices per ACP agent. Rejected,
      unconfirmed, or late superseded choices cannot replace those defaults.
      A provider change retains its confirmed model/provider pair.
- [x] Restore provider before model, before processing queued first prompts.
      Droid's empty acknowledgement is not confirmation: wait for reported
      options, bounded by the existing 30-second RPC deadline.
- [x] Report missing/rejected/unconfirmed saved choices and block prompts
      instead of silently using an adapter default. Keep the connection open
      so available choices can be selected. Repairing one choice does not
      release prompts while another requested identity choice remains invalid.
- [x] Existing conversations restore their own saved identity choices, not
      another conversation's defaults. New-session preferences do not inherit
      permissive modes. Existing native endpoint/model settings and documented
      per-run environment overrides retain their semantics.
- [x] Regression coverage: new session, `/clear`, restart, wrong-agent empty
      reuse, native endpoint/model retention, rejected choices, permission-mode
      isolation, late background confirmation, provider/model coherence,
      queued-first-prompt ordering, missing choice recovery, confirmation
      timeout, and restored conversation/default precedence.

## Verification

The offline/locked workspace run passed with ACP 67, agent 150, app 122,
UI 211 and terminal 17 tests, plus the integration/doc tests. A separate
serial UI run passed 211/211 (156.76 seconds test runtime). The final ACP-only
review refinements passed all 68 ACP tests; formatting, strict workspace/
all-target Clippy, release build and whitespace checks passed again afterward.
Those refinements keep prompts blocked until every requested identity choice
is valid, and keep the actual terminal reason on the collapsed output's last line.
No app source changed between the full UI run and this follow-up.

Evidence: `results.json`, `followup-results.json`, their named logs,
`source-frozen.json`, `source-final.json` and `scoped-final.diff` under the
checkpoint directory. `verify.py --followup` reproduces the final ACP follow-up;
plain `verify.py` runs the full suite against the current source.
The release binary is `target/release/flint`; the user's running app still needs
a restart. The later native follow-up and current verification counts are
recorded above; the following table retains the original live rollout checks.

- [x] `cargo test -p flint-acp` and `cargo test -p flint-app --lib` pass
      offline/locked with an isolated home.
- [ ] With the rebuilt flint and a Droid session in `~/Website`:
      `echo ok`, `npm test` and a heredoc all run and stream output.
- [ ] With a Droid session in `~/yeti-trader` (symlink): `npm test` runs.
- [x] Verify Git operations against a disposable local remote: a shell-line
      ACP terminal in a symlinked workspace pushes and verifies a synthetic
      blob tag. No GitHub remote, credentials or commit identity is involved.
- [x] A deliberately bad command (`nonexistent-bin`) shows flint's real
      reason in the native UI, even when the offline adapter reports "Unknown
      error". Evidence: `native-work-expanded-after.png` in the `-05` native run.
- [x] A transcript with a multi-line heredoc command (success and failure)
      shows one clean row per call, with nothing overlapping. The full
      command appears only when the row is expanded. Inspected native captures:
      `native-work-expanded-after.png` and `native-heredoc-expanded-after.png`
      in the `-05` run; headless bounds assertions also pass.

## Not the cause (ruled out)

These are historical observations from the initial investigation. Real
credentials and external Git operations were not rechecked during this pass.

- **flint's macOS sandbox:** only applies to flint's own `run_command`, not
  ACP terminals. Under the same profile, the shell runs and git can read the
  `gh` credentials.
- **GitHub auth:** `gh` is logged in. `git credential fill`, `ls-remote`,
  `fetch` and `push --dry-run` all work.
- **Nested `sandbox-exec`:** works.
