# Hooter overnight improvement loop

## Authority and deadline

- Requested: improve the orchestrator iteratively, integrate Google's actual
  GenUI/Flutter SDK, and support gated on-the-fly self-improvement.
- Deadline: **2026-10-14 06:00 EDT / 10:00 UTC**.
- No further design questions; choose bounded local implementations.
- Recurring session loop: `bd44331e`, paused at 05:18 EDT and **cancelled at
  the hard stop**.
- Absolute-deadline checker: `db49f9ed`, every five minutes, comparing UTC to
  `2026-10-14T10:00:00Z`. The timezone-ambiguous wall-clock loop `394934f4`
  was cancelled. Do not infer the deadline from the UI's local cron label.
- **Hard stop reached at 10:00 UTC. Both `db49f9ed` and `bd44331e` are cancelled.**
  No further implementation, research, builds, tests or GUI attempts will run.
  Final handoff capture: `target/final-handoff-20261014-d72h9c9a`.
  That point-in-time snapshot precedes this loop-close documentation only.
  The incomplete final Rust UI-suite rerun remains an explicit blocker.
- These loops require this Droid session to remain running.
- Reserve 05:45–06:00 EDT for validation and handoff. Stop new implementation at
  the deadline, cancel both loops, and report partial work honestly.

## Preserve and protect

- Preserve existing user edits, including README/assets and concurrently
  modified engine/tests. Read current files before editing.
- Do not push, publish, deploy, alter credentials, expose ports, or replace a
  running app with unsaved work.
- Self-improvement must use isolated candidates, immutable validation gates,
  review, provenance, bounded budgets, rollback, and explicit activation.
- Never mark unresolved work complete just to clear its UI status.

## Current baseline

- **05:06 EDT checkpoint:** native/model GenUI, bounded adaptive hints, and
  review hardening are integrated. Earlier complete offline workspace baseline:
  **1,183 passed, 25 ignored** with serial execution. Strict workspace Clippy,
  formatting and whitespace checks pass. Flutter: **99 tests**, clean analysis
  and formatting. **25 packaging/integrity-gate tests pass**. Latest isolated native
  release and clean-source companion release builds pass.
  New reviewer coverage: **24 focused tests pass**, strict agent Clippy passes.
  The new test is not yet included in a repeated full-workspace total.
- **05:58 EDT final validation:** the new full Rust rerun is **INCOMPLETE**.
  The 240-second run timed out during
  `native_history_restores_a_missing_later_ui_turn_without_inventing_answer_text`.
  That test passes alone (0.35 seconds). A separate whole-blueprint retry timed
  out after 120 seconds during
  `subagent_sidebar_approval_is_visible_and_answers_parent_engine`.
  No assertion failure was logged, but suite-level stall/order interference is
  unresolved. Do not claim a new 1,184-test full-workspace pass.
  Completed final Rust targets report 771 passed and 23 ignored before the
  incomplete blueprint target. Logs:
  `target/final-validation-20261014-casae4us/{rust-tests,blueprint-retry}.log`.
  Retry process group 67331 was owned and stopped on timeout; process inspection
  found no surviving owned validation process. Unrelated emulator/app processes
  were not touched.
  Final strict workspace Clippy, formatting and whitespace checks pass.
  Final **99 Flutter tests**, analyzer/formatter, **25 packaging/gate tests**
  and current anchored artifact verification (32 files, nine links, 623 sources)
  pass. Actual GUI input, live OmniRoute and executable/distribution gates remain
  held. No production code changed during final validation.
- Local versioned bundle:
  `target/local-bundles/20261014-0505-review-budget/Hooter.app`
  (packaged at 05:05 EDT).
  Its 32 artifact hashes, nine exact symbolic-link targets and 623 source receipts
  pass the new read-only verification gate using an independently retained
  manifest SHA-256. Only widget/native reviewer tests changed since 04:29;
  recorded production build inputs are unchanged. All 32 packaged file hashes
  and nine link targets exactly equal the 04:29 bundle, whose Python sandbox and
  genuine Rust-host two-frame smokes pass with runtime PATH `/usr/bin:/bin`.
  Strict signatures pass again. No unnecessary release rebuild or GUI input
  retry was performed for this test-only change.
  It has not been installed or activated. Earlier 03:47, 04:08, 04:29 and 04:44 artifacts
  remain untouched and are superseded.
  Retained manifest SHA-256:
  `7dbdf99197626813f06acd35c836a9c6e4751605bc44a9779ae488b07b1235e9`.
  Do not obtain a replacement expected digest from an untrusted candidate.
- OmniRoute preset and scoped credentials are implemented. Local no-auth mode
  is explicit and loopback-only. Prior validation: 1,084 offline tests passed,
  strict Clippy/formatting passed, optimized Hooter built.
- Live OmniRoute model-list check timed out; live inference is untested.
- Native auto-fix originally stopped at the screenshot's human-review
  requirement. User authorized fixing and independently reviewing until a
  distinct **Verified automatically** result is possible, never
  **Accepted by you**.
- Automatic independent review is implemented and validated. The repaired
  evidence tracker no longer compares every new check against stale reviewer
  receipts. Reviewed dependencies are tracked and drift revokes verification.
- Baseline full workspace: **1,099 passed, 24 ignored**, live/eval disabled.
  Strict Clippy, formatting and whitespace checks passed.
- Flutter **3.47.7**, Dart **3.13.5** installed outside the repository at
  `/Volumes/T7 Shield/mac-offload/toolchains/flutter-20261014`; not added to
  global PATH. Bootstrap process 46597 completed.
- Google GenUI primary references discovered:
  - <https://github.com/flutter/genui>
  - <https://docs.flutter.dev/ai/genui>
  - <https://docs.flutter.dev/ai/genui/get-started>
  - <https://pub.dev/packages/genui>

## Next bounded iterations

Implementation is frozen as of 05:18 EDT. Do not add speculative features or
repeat refused GUI input. The deadline checker remains active and owns the final
validation/handoff window beginning with its first run at/after **09:45 UTC**.
The remaining gates are real macOS AX/action input, live OmniRoute inference,
publisher signing/notarization and distribution entitlement review, and an
isolated executable-candidate supervisor with external immutable gates and
rollback. The latter is **not implemented**; data-only reminders and local
checksum verification do not substitute for it.

### Final validation, no implementation

Run from the checkout with bounded command timeouts, finishing before 10:00 UTC:

- `cargo test --offline --locked --workspace --all-targets -- --test-threads=1`
  (offline/mocked tests only; live/eval/real-helper tests remain ignored).
- `cargo clippy --offline --locked --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, and `git diff --check`.
- Flutter `test --no-pub`, `analyze --no-pub`, and
  Dart `format --output=none --set-exit-if-changed lib test`.
- `python3 -B -m unittest discover -s tools -p 'test_*hooter_genui.py'`.
- Verify the current 05:05 bundle with its retained manifest digest above.
  Report source drift or gate failure, do not silently replace its anchor.
- Record exact new totals, ignored gates and scoped source fingerprints. No
  production rebuild is needed if only the already verified test sources changed
  and all artifact hashes remain equal to the smoked build.
- At/after 10:00 UTC, stop work, stop only known owned temporary processes if any
  remain, cancel both `db49f9ed` and paused `bd44331e`, checkpoint and hand off.

### Earlier completed iteration priorities

1. Consider only concrete remaining local correctness/quality blockers, avoiding
   speculative refactors or executable activation. Artifact/signature/entitlement
   checks and SDK-independent packaged-helper Rust-host smoke now pass.
2. Actual GUI action verification remains blocked. The earlier optional-action
   smoke acknowledged both mounted frames but failed after its 90-second action
   wait because Cua could not resolve the exact window for background dispatch.
   Do not count it as passed. Any further attempt must use only an owned fixture
   and exact fresh window binding, with no unrequested foreground takeover.
3. Source/binary candidates still need isolated current-byte snapshots, external
   fixed gates, containment, reviewed digests and held activation. Do not
   overwrite this dirty source tree or the running Hooter executable.
   Implemented adaptive hints are fixed data-only reminders, not executable
   self-modification or an executable candidate supervisor.
4. Continue useful bounded iterations until 05:45, then final validation and
   handoff only. Distribution signing, notarization and temporary-entitlement
   distribution policy remain separate, unperformed gates.

## Research and completed lanes

- Google's SDK is **highly experimental**, source package `genui 0.10.5`,
  A2UI `v0.9`, commit `3a9721bf57d5e45877ba2093379c92b522c77392`.
  Chosen architecture: controlled Flutter macOS companion, not embedded GPUI.
  Explicit six-widget catalog, no functions/media/Markdown/URL loading.
- SDK research agent `74412bb2` completed. Source APIs are `SurfaceController`,
  `Surface`, and core A2UI messages, not historical `GenUiManager`.
- Flutter worker `65362c54` completed, owns only `genui/`. **97 tests**, clean
  formatting/analyze, release **42.1 MB**, provider-free executable smoke:
  authenticated, acknowledged two rendered snapshots, rejected unsafe ingress.
  Artifact:
  `genui/build/macos/Build/Products/Release/hooter_genui.app`.
  Sandbox retained; narrow UDS rule under `~/.flint/genui-runtime/`, no general
  network client/server or release debugger entitlement. Distribution policy
  for the narrow temporary exception remains unreviewed.
- IPC worker `a4c20585` completed, owns `genui_bridge.rs`,
  `genui_bridge_tests.rs` and app lib declaration. **20 tests** passed.
  Strict Clippy blocked only by currently unused API; supplemental lint passed.
  Private UDS, peer UID/PID, fresh capability, bounded queues/timeouts,
  30Hz coalescing, replay/target guard, owned-child teardown.
- Orchestration research `bfbb772a` completed. `orchestration` is a durable
  B1 **acceptance-only** ledger; classic `spawn_session` actually executes.
  Do not claim B1 executes jobs or classic queue has exactly-once effects.
  Do not open B1 on a live classic history directory.
- Reuse private descriptor-relative filesystem/writer lease helpers inside
  flint-agent for the improvement registry. Existing evaluator/supervisor
  reference: `tools/blueprint/orchestrator.py`; historical `docs/SLICE.md`
  and `docs/OVERNIGHT-CONTRACT.md` describe absent supervisor code, not a
  working overnight facility.

## Evidence log

- 2026-10-14 ~00:05 EDT: `cargo check --workspace --all-targets --locked`
  passed after fixing optional usage accounting in the independent reviewer.
- Automatic reviewer: **14 new native tests** cover clean semantic review,
  protected coverage, duplicate/unknown evidence, fixes followed by re-review,
  persistence, dependency drift, interruption, forbidden tools/credential paths,
  unstructured output and bounded steps. **15 auto-fix UI tests** passed.
- Native GenUI schema: **nine tests**, strict flint-agent Clippy passed.
  Frame/graph/text limits, UTF-8/JSON nesting/duplicate headers, unknown fields,
  safe bindings, no privileged actions/functions/URLs-as-widgets, tree-only
  graphs (exponential shared-child expansion rejected), redacted debug.
- 01:38 EDT checkpoint: fixed **Stop before TurnStarted** at both layers.
  New per-accepted-input `StartFence` tokens survive interrupted bootstrap;
  active Stop does not poison pending/future requests. Mid-turn steering drops
  its pending token; task controls retain their original semantics.
  Four fence unit tests, deterministic blocked-bootstrap native test, and
  pending-start desktop Stop test passed. Strict flint-agent Clippy passed.
  Workspace all-targets check passed with expected unused GenUI API warnings.
- No agents remain active at this checkpoint. No commits, deployment, live
  inference, installed-app replacement, or running-app restart occurred.
- 02:46 EDT: native/model GenUI and adaptive hints integrated; **1,173 passed,
  25 ignored**, strict checks passed. Registry: 29 focused tests; native adaptive
  integration: three tests. Real Rust host required two genuinely mounted Flutter
  frame acknowledgements. Native UI staging preserves drafts and attachments,
  never submits, and rejects hidden/pending/subagent tasks. Immediate focus
  fences reject switch-away/back stale actions without waiting for a render.
- Three independent simplify reviews covered the original 94 task files against
  base/merge-base `375d93357b492e217e7d55dcb486e94ecefc2f0b`. Existing settings/
  task defaults and endpoint-scoped legacy credentials have real base provenance.
  Separate trust-boundary validators, permission/focus fences, private storage,
  bounded hashes and fixed hint policy were deliberately retained. The custom
  hex formatter was replaced with SHA-256's existing lowercase formatter.
- 03:36 EDT: **23 focused reviewer tests** pass. Reviewer credential filtering now
  checks canonical identities, rejects directory searches and Unix hard links,
  attributes equivalent spellings to persisted evidence, checks model-response
  budgets before tools, hashes off the async thread, and withholds output when
  revalidation fails. Cancellation does not wait for a blocked fingerprint.
- Native GenUI cache uses task projection/content generations and visibility,
  not only task/work metadata; unchanged renders skip surface cloning and
  disconnected views skip rebuilding. Custom native application home no longer
  controls the sandbox-authorized renderer runtime. **Five native UI tests** pass.
- Direct-socket support existed only inside this change and was removed; Dart,
  smoke and native production now share exact `g-XXXXXX/s` launch paths.
  Latest **98 Flutter tests** and analysis/formatting pass. Buttons have readable
  label contrast and minimum 44-pixel height with a dedicated contrast regression.
- The original manifest omitted the XIB and seven template icon PNGs. They have
  been inspected and explicitly included, along with the new task projection
  cache, in a **103-file**, 859,109-byte complete scoped diff:
  `target/review-20261014-postfix-gvvgd65m/manifest.json`.
  Root README/assets, credentials and live configuration are excluded.
- Clean companion source contains exactly the inventoried source files:
  `target/genui-clean-20261014-sqcbg5a3/source`.
  Offline lock-enforced dependency resolution, release build, strict signature
  check and two mounted-frame/unsafe-ingress/EOF smoke pass. Xcode emits upstream
  AVFoundation deprecation and script-output warnings; this is not a warning-free
  dependency build. No SDK source patch or dependency override was introduced.
- Packaging source receipts now include Swift, Xcode configs/projects/workspace,
  XIB, icon assets and Flutter native registration inputs, excluding user-local
  Xcode metadata. Outside-checkout source aliases are rejected. **Seven safety
  tests** pass. Local outer signing is ad hoc; no distribution identity,
  notarization, installed-app change or executable activation was performed.
- 04:05 EDT: a final persistent Cua transport confirmed exact owned helper PID/
  window ownership but returned zero AX windows and explicitly refused all
  background input (`ax_window_unresolved`). No click, foreground takeover or
  desktop input occurred. Required-action fixture waits failed; actions remain
  unverified. Temporary fixture/transport processes exited.
- The screenshot exposed compact desktop density shrinking the painted button
  from the declared 44-pixel minimum to **36 pixels**. A regression measured the
  actual Material bounds under compact density and reproduced the failure.
  The button now explicitly uses standard visual density. Actual bounds are
  **44 pixels**, and all **98 Dart tests**, analysis and formatting pass.
  Complete final companion source:
  `target/genui-final-20261014-bb5ypvv6/source`.
  Its clean release/signature/sandbox smoke passed; the updated local bundle and
  both packaged-helper smoke routes pass. No more background GUI attempts are
  planned without new evidence or explicit foreground-control authorization.
- 04:32 EDT: added `tools/verify_hooter_genui.py`, a bounded, read-only consistency
  gate requiring a caller-supplied manifest digest retained outside the candidate.
  It verifies strict receipt schema, source/file checksums, exact contained link
  targets, regular/non-hardlinked inputs, budgets, strict signatures and the
  narrow helper entitlements. It detects ordinary artifact drift during signature
  inspection, never executes the candidate and never signs, installs or activates.
  **25 mocked packaging/gate tests pass**, including manifest tampering, internal
  link retargeting, outside links, FIFOs, path aliases, source/file drift, signature
  and entitlement failures. The packager now rejects an absolute link that was
  internal before copying but escapes the new bundle after copying.
  The new separate versioned bundle passes the actual verifier, Python sandbox
  smoke and real Rust-host two mounted-frame acknowledgements. Gate source/test
  code is included in source receipts. These are local checksums, not proof that
  binaries were built from those inputs, signed build attestation, publisher
  identity, activation-time immutable isolation or a rollback supervisor.
- 04:45 EDT: inspected the pinned Flutter `SemanticsBinding` implementation and
  companion Dart/Swift bootstrap. Flutter already mirrors platform-requested
  semantics; no concrete bootstrap defect explains the unresolved macOS AX
  window. Do not force semantics globally or retry refused input speculatively.
  Added a genuine SDK-backed widget regression asserting both button labels,
  enabled/button semantics and tap actions; simulated semantics activation
  forwards only the fixed `open_task` / `stage_prompt` action identities.
  The first test run exposed late disposal of a test-only semantics handle;
  explicit `finally` disposal fixes the harness. **99 Flutter tests**, analyzer,
  formatter and whitespace checks pass. This is framework semantics evidence,
  not real macOS AX input, user clicking, provider inference or human acceptance.
  Repackaged only to refresh the recorded test digest; artifact bytes and links
  are identical to the already smoked build. The refreshed gate passes.
- 05:06 EDT: inspected cancellation and fingerprint budgets. `task::file_state`
  already limits each file to **8 MiB**, both before reading and during growth.
  No unbounded hashing defect was found; no production change was justified.
  Added a sparse oversized-file regression proving both reviewer `read_file`
  and `grep` fail before tool start or another provider request. All **24 focused
  reviewer tests**, strict agent Clippy, workspace formatting and whitespace pass.
  Async cancellation returns promptly; an OS-blocked filesystem operation may
  still finish separately. This is not an OS-process isolation guarantee.
  Refreshed test receipts in a separate bundle; all artifact hashes/links still
  equal the already smoked build and the strict anchored gate passes.
