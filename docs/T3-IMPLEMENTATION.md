# T3-style chat and orchestration implementation

## Authority and boundaries

The user requested implementation in Flint on 2026-10-05 and selected the full
chat and orchestration scope, including new features. The user approved Factory
workers and local checks after the older Cursor/OpenCode loop configuration
failed its availability preflight.

This is a new implementation pass, not a promotion of the earlier overnight
experiments. Their frozen contracts, historical results, and artifacts remain
unchanged. Preserve all starting tracked and untracked work, especially the task
outcome/review implementation.

Keep the native Rust/GPUI application and its dark-and-ember design. Adapt the
useful T3 behaviors rather than transplanting its React stack or silently
replacing the application architecture.

Do not commit, push, publish, use real provider credentials in tests, weaken
approvals or filesystem protections, or activate/restart/replace the running
user app. Use disposable homes/workspaces and a pass-owned Cargo target.

## Starting evidence

- T3 source examined through `4df84a7d03ff084ff4c6dd7aef54623984690fb7`.
- Stable T3: `v0.0.45`. Nightly examined:
  `v0.0.46-nightly.20261005.2702` (`cfa4f765ec05`).
- Original source/status/patches and binary hashes are retained in
  `/Volumes/T7 Shield/mac-offload/flint-t3-implementation-glct94hg/`.
- A recurring 15-minute session checkpoint tracks useful in-scope work. It is
  not a parallel builder or a replacement for implementation and validation.
- The existing application already has virtualized transcripts, subagents,
  queues/steering, worktrees, task-review outcomes, and ordered event writes.
  Preserve and extend them instead of adding competing implementations.

## Completion gates, frozen before implementation

1. Every feature has a real entry point, persisted state where required, and
   observable behavior. A detached helper or undocumented unsupported button
   does not count as implementation.
2. Changed engine behavior has focused offline tests. Changed UI behavior has
   real-view interaction tests, including narrow panes where applicable.
3. Streaming coalesces only replaceable progress. Never drop terminal events,
   accepted instructions, approvals, child results, or persisted event order.
4. Retried command identities cannot duplicate accepted work. Recovery holds
   queued work rather than starting it without consent. Stops propagate without
   confusing root completion with child completion or finalization.
5. Existing task evidence/review/acceptance gates remain authoritative. A model
   completion, UI action, provider handoff, or replay must not certify a task or
   silently bypass a review hold.
6. Provider capabilities are explicit. Native steering and ACP stop-then-send
   remain distinct. Unsupported provider operations fail visibly before changing
   conversation or workspace state.
7. Context transfer is bounded, preserves the current request, attributes
   historical messages, and offers retrieval of omitted history. It does not
   claim native reasoning/tool-state parity.
8. Secret values must not enter chat/persistence/debug output. Rich HTML or
   diagram content cannot reach the app's credentials, session, or filesystem
   through an unrestricted embedded renderer.
9. No continuously repainting decoration, whole-history rebuild per token, or
   unbounded detail copying. Performance claims require retained measurements;
   passing tests or budgets alone is not a speedup.
10. Run focused tests during each slice. Final integration includes formatting,
    strict affected-crate Clippy, offline workspace tests with serial UI,
    an isolated release build, and whitespace/diff review. Retain failures as
    well as subsequent passing results.
11. Inspect fresh native screenshots for implemented visible features using the
    isolated binary and disposable homes, sequentially and without activation.
    Physical keyboard, VoiceOver, and live providers are separate unverified
    surfaces unless actually exercised with authorization.
12. An independent read-only review checks the final intended changes against
    the starting source snapshot. Resolve verified defects and rerun affected
    gates before claiming completion.

## Implementation tracks

The audit and builder PHASE 0 define exact APIs, file ownership, and bounded
slices before editing shared contracts.

### Chat and interaction

- Stable incremental Markdown/code rendering and scroll behavior.
- Rich replies, diagrams, tool images, and bounded/on-demand details.
- Context quotations/citations, prompt history/stash, editable queued work, and
  appropriate history-rewind behavior.
- Chat-width/readability controls and direct child-conversation interaction.
- Private secret-request UI integrated with native tools and approval rules.

### Durable orchestration

- Lightweight navigation summaries separate from transcript detail/history.
- Engine-owned durable queues and idempotent command receipts/side-effect intent.
- Event-driven child/task waits and bounded streaming delivery.
- Cross-provider delegation, explicit lineage/capabilities, cancellation and
  restart recovery.
- Budgeted provider handoffs/forks and safe checkpoint/history coordination.
- Git/PR watch notifications where supported, without poll-induced process or
  provider churn.

## Loop protocol

Finish one bounded slice before starting the next. Builders document substantive
PHASE 0 disagreements and verify real Rust/GPUI/provider APIs. Shared contracts
are frozen before parallel ownership begins. Builders report raw commands,
exits, counts, and artifacts; the primary agent independently judges them.

Do not start overlapping builds, native scripts, or workers on the same files.
Stop a slice on a real blocker, retain evidence, and resolve it explicitly.
Cancel the recurring checkpoint when implementation and final validation finish.

## Progress and results

Current checkpoint (historical raw results remain below):

The later user instruction is **just finish this up, do not continue the loop**.
Checkpoint `1618659f` is cancelled. `T3-CLOSEOUT.md` authorizes only bounded
current-source regression recording, not another feature stage. Full scope
remains incomplete; the active frontend proposal may finish without code/test/
launch authority, and the completed backend corrections stay unimplemented.

Fresh closeout is recorded in `T3-CLOSEOUT-RESULTS.md`: strict workspace Clippy
passes; formatting fails; ACP 74, agent 282/1 ignored and app units 201/21 ignored
pass; UI is 323 passed/2 failed; B1 preserve_order is 37 passed. No source drift
or original-binary change. All owners are idle, both new proposals remain held,
and no loop is scheduled. The full requested implementation is not complete.

- F1 is independently accepted. F2a returned four focused passes/one failure:
  pointer Capture clears transcript selection before width/button handlers.
  Primary app units pass 201/21 ignored and strict app lint exits 0; selection/
  action/formatting/full-UI gates remain pending. Native PHASE 0 has returned;
  `T3-CHAT-F2A-NATIVE-RULINGS.md` permits only same-owner N0 verified copies/
  relative root coordination/full-copy tooling/strict baseline. N0 returned:
  31 mocked tooling passes, production check 0; Fresh Base Clippy has no
  selected driver proof, Pre pending. Narrow native-lint follow-up and N1
  still need review/freeze. Primary subsequently verified all provenance/root/
  raw identities and independently reran 31 mocked tooling passes.
  `T3-NATIVE-LINT-PHASE0.md` authorizes only a same-owner actual-driver proposal.
  Selection API/app/action/highlighting stay held.
- B1 is independently accepted for bounded progression: repaired source review,
  37 focused/mode tests, strict lint and the final settled-source 282-agent/
  74-ACP full regression pass with all guards unchanged. Same-owner B2 PHASE 0
  returned 12 disagreements; `T3-ENGINE-B2-RULINGS.md` selects B2a and requires
  its exact private schema contract before code. That proposal has returned;
  `T3-ENGINE-B2A-CONTRACT-REVIEW.md` accepts scoped stated limits but requires
  four corrections and a smaller blob-codec first-stage contract. No
  scheduling/provider execution or B2 implementation is authorized yet.
- R0E is complete and independently checked: 57 mocked tests pass; E-A/E-B
  reach main and deny synthetic file opens/actual loopback connect with EPERM,
  actual controller/child exits 0 and all sealed artifacts/guards/timing checks
  pass. R0=34/R0D=8/R0E=2, cumulative=44. This is native diagnostic evidence,
  not WebKit/HTML/Mermaid capability. Same-owner follow-up PHASE 0 returned;
  `T3-RENDERER-W1-REVIEW.md` holds W1 for positive service ownership/teardown,
  no-prompt and enforceable aggregate containment. Research has now completed:
  `T3-RENDERER-CONTAINMENT-VERDICT.md` accepts only a bounded negative shared-
  Mac finding and retains the full-renderer hold. A different-platform
  research versus hold decision has now been answered: the user selected
  **Hold renderer; continue chat/engine**, frozen in `T3-RENDERER-HOLD.md`.
  No new renderer research, permission or launch.
- Older checkpoint 3b2e55bc remains cancelled; checkpoint 544a56d2 is now
  cancelled at the new containment input blocker, now resolved by explicit
  renderer hold while chat/engine continues. Research is completed and reviewed;
  W1 code/execution stays held.
  Useful chat/engine checkpoint is `1618659f`; renderer owner remains idle.
  Frontend N0 has returned the bounded prerequisite result above.
  Backend exact-schema review is still pending; no B2 implementation,
  environmental creation/acquisition or new renderer launch is authorized.
  Existing frontend/backend invocations may finish only their current scopes.
  No full implementation completion claim.

- Source preservation: 235 files, 3,249,894 bytes.
- Cursor preflight: exit 1, configured model unavailable on current plan.
- OpenCode model listing: zero entries; original reviewer not selected.
- Factory worker/local-check substitution: explicitly approved by the user.
- Current feature audit: completed against the dirty source.
- Initial offline agent unit baseline: 242 passed, 0 failed, 1 ignored.
- Initial offline app unit baseline: 195 passed, 0 failed, 21 ignored.
- Initial serial UI baseline: 303 passed, 0 failed, 0 ignored; exit 0.
- Pass-owned validation runner: syntax/help, credential/focus environment
  filtering, and original release-binary hash guards checked.
- Frontend PHASE 0: reviewed; 12 substantive disagreements accepted with
  boundaries in `T3-SLICE-CHAT-01.md`.
- Chat slice 01: approved for retained Markdown/resource safety without new
  dependencies or shared backend type changes.
- Backend PHASE 0: reviewed; eleven disagreements ruled on in
  `T3-CONTRACTS.md`, with full isolated rendering selected by the user.
- Engine slice 01: durable acceptance/recovery foundation approved in
  `T3-SLICE-ENGINE-01.md`; execution capabilities remain disabled until B2.
- Full isolated rendering bridge: user selected this instead of native partial
  HTML/flowcharts; a separate PHASE 0 checks packaging, real APIs and isolation.
- Validation tooling regressions: 15 passed/0 failed. Checks now retain source
  fingerprints, guard frozen contracts, and exclude provider/SSH/focus/telemetry
  environment inheritance. Final commands and native isolation are recorded in
  `T3-VERIFICATION.md`; this does not claim feature validation.
- Renderer PHASE 0: reviewed; a bounded non-production WebKit isolation probe
  is approved in `T3-SLICE-RENDERER-00.md`. No Mermaid acquisition or production
  untrusted interactive HTML is enabled before the isolation ruling.
- Expanded validation tooling: 17 passed/0 failed, including frozen addenda and
  renderer-source provenance. Existing blueprint tooling: 95 passed/0 failed.
  Fresh binary/contract guards report unchanged. These are not feature results.
- Current F1 raw checks: app unit 199 passed/0 failed/21 ignored; full serial UI
  316 passed/0 failed/0 ignored. Earlier failing interaction checks are retained.
  F1 independent review and final lint gate remain pending; no slice verdict.
- Current dependency coordination: initial locked engine/app checks failed
  because the approved agent manifest additions required lock reconciliation.
  Offline resolution completed dependency lookup but the new engine foundation
  reported five missing-import errors and one unused import. B1 is still in
  its build-test-fix loop; the failed logs/results are retained.
- Latest checkpoint: no duplicate workers or overlapping builds started;
  F1/B1/R0 remain the current bounded slices. Frozen contracts and original
  binary guards pass, and tracked whitespace check exits 0.
- Next chat slice gates prepared in `T3-SLICE-CHAT-02.md`: persisted readability
  width and safe Copy/Stage code actions. It is not started; F1 independent
  acceptance is required first. Highlighting remains a separate dependency
  feature slice, not a language-label claim.
- F1 independent review: repair required for selected/streaming cache entries
  whose archived owner is removed. A targeted parent/child archive/undo
  regression and owner retirement are authorized within F1, not a new slice.
- Independent strict app Clippy now reaches actual lint after dependency
  reconciliation; exit 101 on one new test's useless `String` conversion.
  Source, frozen contracts and original binary hashes were unchanged during
  that check. The original worker is repairing both findings before F2a.
- Renderer R0 raw results: five successful compiles, one failed compile, and
  34 sandbox launches exiting SIGABRT before helper output. No raster or
  executed negative-security result exists; zero canary hits are inconclusive.
- Renderer startup review: the matching minimal-C and Swift crash reports stop
  in the system dynamic loader before application entry. The missing operation
  is unknown. `T3-SLICE-RENDERER-00D.md` authorizes a bounded native-only loader
  diagnostic, not WebKit retries, Mermaid acquisition, or production rendering.
- Next engine slice is prepared in `T3-SLICE-ENGINE-02.md`, not authorized.
  B1 acceptance and a concrete native-executor integration proposal are required
  before freezing B2 execution gates. It does not enable scheduling.
- Actual syntax-highlighting preparation is in `T3-SLICE-CHAT-02B.md`.
  The required GPUI grammars are not cached. Its raw-language default cache also
  needs a bounded override before enabling the feature. No acquisition,
  manifest/lock change, or highlighting capability has been authorized yet.
- Validation source fingerprints now include renderer C/header/HTML/CSS/module
  inputs, not just Swift/scripts. Expanded mocked tooling checks: 20 passed/
  0 failed, with actual document mutation rejected by the stable-source gate.
  Earlier evidence retains its original, narrower fingerprint coverage.
- R0D PHASE 0 is reviewed in `T3-RENDERER-R0D-RULINGS.md`: four narrow profiles
  and at most eight native launches are authorized. The requested exact-root
  directory data-read exception is not authorized; D5 is omitted. Full renderer
  capability remains disabled and no WebKit follow-on is implied.
- B1 worker completed: agent 271 passed/1 ignored, ACP 74 passed; strict
  checks/formatting exit 0. Three agent tests are concurrent JEV work, not B1.
  Independent review holds acceptance for two repairs in
  `T3-ENGINE-B1-REVIEW-RULINGS.md`: remove new enum-size warning allowances by
  boxing new payload variants, and explicitly sort canonical JSON keys so
  immutable command digests do not depend on serde_json feature unification.
- Additional B1 source review holds acceptance for canonical receipt-lineage
  validation before recovery commits. The counterexample and required
  actual-store corruption regressions are in `T3-ENGINE-B1-RECOVERY-REVIEW.md`.
  This gate queues behind the current same-worker repair, not a duplicate job.
- R0D worker returned: one warning-clean C compile, 39 passing mocked tests,
  two pre-output aborts and one version-3 launcher permission error. No `main`
  or executed native deny result. The classifier stopped before D3/D4; a
  scoped continuation authorizes only five unused slots, unchanged profiles/
  permissions and cumulative eight-launch/600-second bounds.
- F1 repair worker returned app units 199 passed/21 ignored and clean strict
  app lint. Its complete 17-test focused Cargo pass had unrelated renderer
  source drift; a stable 15-test run omitted the two stress cases. Primary
  independent full-focused/unit/lint validation is now running in the same
  serialized isolated slot, not a second frontend worker.
- Primary F1 repair checks completed: full focused UI 17 passed/0 failed;
  app units 199 passed/21 ignored; strict app lint exit 0. All three source,
  frozen-contract and original-binary guards are unchanged. F1 is accepted for
  bounded progression in `T3-CHAT-F1-ACCEPTANCE.md`; the existing frontend owner
  may now implement F2a, not highlighting or renderer capability.
- B1 first review repair passed 28 focused tests in both normal and
  preserve_order modes, agent 273/1 ignored and ACP 74. Strict Clippy still
  failed on the now-larger Progress variant. `T3-ENGINE-B1-FINAL-REPAIR.md`
  authorizes boxing that new variant and completing the frozen receipt-lineage/
  stopped-mutation corruption gate, without suppressions or B2 execution.
- R0D continuation is complete: five additional launcher EPERM failures,
  eight cumulative native launches and no main/native-denial output. Primary
  verified all 70 new/55 old artifacts and reran 52 mocked tests (exit 0,
  zero native launches). `T3-RENDERER-R0D-VERDICT.md` holds renderer capability
  and further launches pending a user decision; no grants or old gates changed.
- The user explicitly approved an exact-root-directory diagnostic. New
  `T3-SLICE-RENDERER-00E.md` freezes a version-1 baseline plus that sole directory
  permission bundle, at most two new native launches/300 seconds and no compile/
  WebKit/fallback. Same renderer owner is proposal-only pending rulings. Old
  R0D slots/gates remain exhausted and unchanged. Recurring checkpoint resumed
  as `3b2e55bc`; F2a continues and B1 final repair awaits independent review.
- R0E PHASE 0's twelve disagreements are ruled on in
  `T3-RENDERER-R0E-RULINGS.md`. The same renderer owner may implement only three
  new source files; tests/native execution require primary review and an external
  hash seal. Exact candidate/limits are unchanged, with conservative first/
  nonblocking-stage failure stop and no root enumeration or additional grant.
- R0E source-only implementation and primary static review have finished without
  execution. The queued same-owner F2a action corrections are recorded in
  `T3-CHAT-F2A-ACTION-REVIEW.md`; the native-selection proposal remains active.
  No duplicate worker/build, installed-cache change or relaxed gate was used.
- The separately sealed R0E finite invocation later completed with actual exit
  0, 57 tooling passes and two complete native open/connect denials. Primary
  independent verification checks all 35 immutable artifacts/790 preserved
  hashes, fixture/canary/process evidence, 44 cumulative consumed launches and
  released-lock/final timing. `T3-RENDERER-R0E-VERDICT.md` permits follow-up
  proposal only; full renderer and any new WebKit/permission experiment remain
  separately gated. B2 is proposal-only after independent B1 acceptance.
- B2's twelve disagreements are ruled on, with exact schema proposal-only
  dispatched to the same owner. W1's fifteen disagreements expose a genuine
  shared-host containment blocker; a user decision on bounded disposable-
  containment research versus renderer hold is required before that track
  continues. Chat/engine proposals remain authorized and no gate is weakened.
- The later user answer authorizes disposable-containment research only.
  `T3-RENDERER-CONTAINMENT-RESEARCH.md` specifies one same-owner evidence-backed
  proposal and preserves the no-creation/no-acquisition/no-execution boundary.
  The completed six-disagreement native-selection proposal and exact B2a
  contract are under architect review, not accepted implementation authority.
- Native-selection D1–D6 have a staged ruling. N0 is mechanical provenance,
  exact offline root coordination and uncapped native baseline only; no app
  policy change. Valid visible tiled Copy must remain distinct from active-pane
  Stage/selection eligibility. N1 waits for independently reviewed N0 evidence.
- Complete B2a contract review requires corrected predecessor revision rules,
  v1/v2 stopped-task/queue semantics and zero-publication preflight. The next
  proposed typing scope is only the test-owned immutable blob codec, not the
  entire private executor/JSON scanner. The same backend owner is contract-
  correction-only until primary freeze and serial source coordination.
- Containment research completed with twenty public searches, local SDK facts
  and one report, not environment execution. All twelve disagreements are
  ruled on in `T3-RENDERER-CONTAINMENT-VERDICT.md`; the guest candidate lacks
  hard aggregate host-resource/all-state teardown guarantees. Dedicated-Linux
  proposal-only research needs a separate user decision; no unsafe fallback.
