# T3 implementation verification

These are intended checks, not passing results. Frozen completion gates remain
in `T3-IMPLEMENTATION.md`; this document does not relax them.

The user-requested stopped closeout ran four checks once, serially. Actual
results are `T3-CLOSEOUT-RESULTS.md`: formatting exit 1, strict workspace Clippy
exit 0, workspace tests exit 101 (UI 323 passed/2 failed), B1 preserve_order
37 passed. All preservation guards pass. Release/native/uncapped-native-lint
gates below remain unrun. No loop or owner invocation remains active.

## Isolation and provenance

Run root:
`/Volumes/T7 Shield/mac-offload/flint-t3-implementation-glct94hg`.
All Cargo commands use its `specs/validate.py`, one file-locked build slot,
fresh private homes, explicit offline mode and the pass-owned target.

The runner rejects alternate manifest/target/config paths, real-shell provider
and telemetry/SSH/focus environment inheritance, changed original binaries,
and altered frozen contracts. It retains failures, command logs, source hashes
before/after, and guarded-binary hashes. Final checks use
`--require-stable-source`: a source change during validation is not a final
passing checkpoint.

Tooling validation: `specs/validate_tests.py`, 17 tests passed/0 failed.
Evidence: `evidence/validation-tooling-final-1791254660175351000.log`.
Expanded renderer-input provenance validation: 20 passed/0 failed, including
actual C/header/HTML/CSS/module changes and mid-check document mutation.
Evidence: `evidence/validation-renderer-inputs-1791256041055532000.log`.
New fingerprints cover these inputs; older fingerprints are not retroactively
expanded or treated as stable integrated-source verdicts.
Existing blueprint evidence-tooling regressions: 95 passed/0 failed.
Evidence: `evidence/blueprint-tooling-baseline-1791254660304954000.log`.
This is tooling evidence, not a chat/orchestration feature pass.

## Focused slice checks

- Engine: transaction/recovery/lease tests; existing persistence/task/approval
  and shared-evidence regressions; local dummy HTTP/adapter fixtures only.
- Chat: pure reducers/resources plus actual `FlintApp` interaction tests,
  parent/child identity, selection/copy, streaming-to-final promotion,
  offscreen rows, bounds, narrow 320/420 px panes and tiled focus routing.
- Queues: immutable identities, unsent-outbox receipt reconciliation, restart
  holds, edit cancel+replace, review holds and explicit one-shot continuation.
- Delegation: native shared WorkClock/undo, correct target credentials, root
  versus child cancellation, finished-before-wait/lost-wakeup tests, and visible
  unsupported ACP policy/capability states.
- Transfer: bounded attributed history, exact current request, omission
  retrieval, stale revisions, held destination lineage, unchanged source logs
  and workspace, no inherited task certification.
- Rich rendering: actual pinned Mermaid/browser runtime, restricted resources,
  full diagram fixtures, malformed/time/size/crash handling, revision-owned
  results, and no credential/session/filesystem/network escape.
- Private requests: masked real input, one-use owner/expiry checks, cancelled
  entities and buffers, no normal draft/stash/automation/clipboard persistence,
  and dummy sentinel absent from logs/history/errors/model/tool text.
- Watches: local read-only HTTP fixtures, normalized fingerprint/ETag,
  backoff/cancellation/stale-response tests, exact unread acknowledgement,
  paused reopen, and no remote writes or task scheduling.

## Final serialized Cargo sequence

If the native path copies are retained after N0/N1 review, final provenance
must include every copied resource/manifest/license/provenance file, not merely
Rust sources. Rerun explicit uncapped `gpui-base` and `gpui-pre` library Clippy
through the same wrapper as required by `T3-CHAT-F2A-NATIVE-RULINGS.md`.
Workspace dependency caps/exclusion do not establish strict affected-native
lint. Check only modified native Rust files with the pinned owned-file
formatting procedure; do not reformat untouched third-party bytes or resolve
the excluded development/bench graphs. These are pending gates, not N0 results.

Prefix each command with the absolute validation runner path, a unique
`--label`, a suitable `--timeout` (maximum 3600 seconds), and
`--require-stable-source --`.

1. `cargo fmt --all -- --check`
2. `cargo clippy --offline --locked --workspace --all-targets -- -D warnings`
3. `cargo test --offline --locked --workspace -- --test-threads=1`
4. `cargo test --offline --locked -p flint-app --test blueprint_ui -- --test-threads=1`
5. `cargo build --offline --locked --release -p flint-app`
6. `git diff --check`, explicitly targeting this repository.

Retain test counts and ignored/skipped surfaces separately. Opt-in live,
benchmark or physical-device tests are not passes just because they are
disabled. Fix regressions without suppressing warnings or changing acceptance
thresholds. Record unrelated starting failures as such with source evidence.

## Native inspection

Use only the pass-owned release binary, never the user's existing executable.
Blueprint scripts require explicit `--bin`, an absolute pass-owned output root,
credential-isolated process environment, disposable homes/workspaces and
`FLINT_BP_NO_ACTIVATE=1`. Never allow their default release-build helper.

Run native scripts/captures sequentially in the same slot as Cargo, without
interacting with the user's window. Window-ID capture targets only the process
launched for this verification. Inspect fresh parent/child streaming, narrow
composer/width, queue/recovery, rich Mermaid/HTML, citation/fork, private metadata
and notification screenshots. Do not capture private values.

The existing blueprint helper inherits its parent's environment, so launch it
from a filtered wrapper, not the user's raw shell. Physical input and VoiceOver
remain separately unverified unless authorized and actually exercised.

## Independent review and closeout

A read-only Factory reviewer compares the intended change to the preserved
`source-before/`, not merely Git HEAD. The primary rules on defects and reruns
affected checks. No worker grades its own feature implementation.

Verify original source backups, historical gates/contracts, original release
binaries and user's running app remain preserved. Update implementation and
handoff docs with actual connected feature scope, raw evidence and limitations.
The older loops `74615eb6`, `3b2e55bc` and `544a56d2` are cancelled, the last
at the containment input blocker later resolved by user-directed renderer
hold. The later user-requested closeout cancels `1618659f`; keep the renderer
idle and do not restart the loop. `T3-CLOSEOUT.md` defines bounded current-source
checks and an honest stopped handoff, not full feature acceptance. R0E consumed its two finite slots;
verification must never repeat them or reset the historical R0/R0D counters.

Current accepted bounded evidence: F1 retained native Markdown/resource safety,
B1 durable acceptance/recovery and R0E bare-C startup/open/connect denials.
Those results do not replace the final sequence or prove full WebKit,
HTML/Mermaid, orchestration execution or remaining chat capabilities. F2a's
remaining Stage fixture-tree evidence is prepared in
`T3-F2A-VALIDATION-NOTES.md`; no pending test is counted as passed.
