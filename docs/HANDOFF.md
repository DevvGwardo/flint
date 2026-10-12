# Flint overnight improvement handoff

## Goal and authority

Requested 2026-10-04: analyze the current codebase and run an overnight loop
focused on performance, orchestration, and reducing technical debt. Add an
oversight layer to the existing blueprint so changes and follow-up opportunities
can be checked against evidence.

The working tree already contains extensive tracked and untracked changes.
Preserve them. The running user app and its executable must remain untouched.
No commits, pushes, publishing, paid live-provider tests, credential export,
permission weakening, or automatic promotion of experimental patches.

Unattended authority covers bounded experiments in isolated source copies,
offline validation, local evidence, and proposing changes for morning review.
It does not cover merging those changes into the user's checkout.

## Current evidence and architecture

- Four Rust crates: native agent, ACP adapter, GPUI app, terminal.
- Current validation history: `STATE.md`, `GATES.md`,
  `docs/IMPROVEMENT-LOOP.md`, `docs/ENGINE-IMPROVEMENTS.md`.
- `docs/IMPROVEMENT-LOOP.md` contains 28 passes. Their measured improvements
  are generally helper-specific; they are not blanket whole-app speedups.
- Production native/ACP/subagent event channels are unbounded.
  Replacing them with bounded channels without redesigning delivery can drop
  events (`try_send`) or deadlock approval/protocol handling.
- `EventWriter` uses an unbounded command channel and retains failed batches.
  Accepted-event ordering, retry retention, barriers and shutdown must survive
  any future storage-pressure work.
- Transcript tool lookup and nested child handling need representative
  long-history/concurrent-update measurements before adding indexes.
- Picker idle CPU remains an intermittent historical finding, not a closed
  causal fix. Physical keyboard, clipboard, IME and VoiceOver are unverified.
- The active user app has child agents. Native performance measurements must
  not be presented as clean-host evidence while other workloads are active.

## Frozen acceptance gates

See `docs/OVERNIGHT-CONTRACT.md`. Freeze a copy and hash it in each run before
any experiment. A builder cannot edit acceptance thresholds or its own verdict.

Every proposed patch needs scope checks, an independently executed offline
validation suite, an independent review, retained raw results, and a source
fingerprint. Failed or missing checks never become passing claims.

## Roles

- Architect: chooses bounded slices and rules on disagreements and evidence.
- Builder: Cursor Agent, sandbox explicitly enabled, isolated experiment copy.
- Reviewer: independent OpenCode reviewer, never writes implementation code.
- Supervisor: deterministic local process, owns deadlines, locks, subprocess
  cleanup, evidence, validation and stop conditions, not engineering judgment.
- Human: reviews the morning report and decides promotion/merge.

## Open disagreements and rulings

1. A literal endless loop would spend resources without proving useful work.
   Ruling: use an eight-hour maximum, a final regression reserve, finite
   candidate count, and stop on blockers or repeated unproductive passes.
2. Committed-HEAD worktrees omit the current uncommitted implementation.
   Ruling: snapshot tracked and relevant untracked source, not just HEAD.
3. Historical passing budgets do not establish a fresh baseline.
   Ruling: measure the snapshot again and label unavailable native evidence.
4. Shared build output can rewrite the open user's executable and contaminate
   before/after binaries.
   Ruling: use a run-owned Cargo target, disposable test home, and retained
   binary hashes; never replace `target/release/flint`.
5. Autonomous acceptance would undermine independent review.
   Ruling: candidates are only proposals, never automatically promoted.

## Next slice

Implement and test the supervisor and blueprint oversight layer described in
`docs/SLICE.md`. No application feature changes in this setup slice.

## Raw results

No new runtime or improvement claims yet. Setup and overnight evidence must be
recorded with actual command exits, source hashes, timestamps and limitations.

## Setup checkpoint and architect rulings

The setup builder ran in
`target/overnight-bootstrap-fkgvgswy`, a copy of 245 current source/history
files, not a committed-HEAD worktree. It produced a prototype and PHASE 0
pushback, then stopped at the Cursor account usage limit. Its implementation
is not yet accepted or installed. A bounded local worker is repairing the
identified safety defects and completing mocked regression coverage.

PHASE 0 dispositions:

1. Accept missing HEAD/binary in bootstrap as truthful unavailable provenance.
   The original repository has a HEAD and a release binary to fingerprint.
2. Accept current-byte snapshots and isolated Git boundaries.
3. Accept fail-closed authentication/sandbox behavior. Never downgrade it.
4. Accept strict independent-review availability/verdict gates.
5. Accept investigation-only queue policy. Dropping accepted events is forbidden.
6. Accept native skips and separation of native/headless focus environments.
7. Accept useful-work bounds, not eight hours of busy waiting.
8. Accept the eight-path cap with explicit separate evidence paths.

Independent preflight rejects `opencode/deepseek-v4-flash-free` as
`Model unavailable`; the installed model-list command returned no entries.
Unattended patch production/acceptance is therefore blocked by both the
builder quota and reviewer availability. No silent model replacement.

A separate, safe oversight loop is scheduled every 30 minutes. Its control
and evidence live under
`target/overnight-oversight-20261005T025720Z/`; it expires by
2026-10-05 06:57 EDT, stops on drift, and never edits application sources.
It audits one bounded area per wake and consolidates actual offline evidence.
This is not a claim that autonomous feature improvements are running.

Next verification: independently run the supervisor Python suite, inspect
the full diff/safety boundaries, exercise a real dry-run, and launch an
offline `--baseline-only` validation without either model dependency.

## New user-authorized T3 implementation pass

The later 2026-10-05 user request authorizes implementation of full T3-style
chat/orchestration behavior in this checkout, preserving the existing dirty
source. The user expressly approved Factory workers and local checks after the
Cursor/OpenCode availability failures. This is separate authority, not a
retroactive alteration or acceptance of the historical overnight experiments.

Current gates and progress: `docs/T3-IMPLEMENTATION.md`.
Current frontend slice: `docs/T3-SLICE-CHAT-01.md`.
Preserved source and raw evidence:
`/Volumes/T7 Shield/mac-offload/flint-t3-implementation-glct94hg/`.
Recurring checkpoint: `74615eb6`, 15-minute cadence, cancelled on completion
or a genuine unresolved blocker.

Initial offline results: agent 242 passed/0 failed/1 ignored; app unit
195 passed/0 failed/21 ignored; serial UI 303 passed/0 failed/0 ignored.
The user's original release binaries and running app remain untouched.
Frontend retained-Markdown/resource-policy implementation is authorized.
Backend durable-acceptance/recovery foundation is authorized by
`docs/T3-SLICE-ENGINE-01.md`; shared contract baseline is
`docs/T3-CONTRACTS.md`. No provider execution is enabled in B1.
User selected full isolated HTML/Mermaid rendering rather than native subsets;
the independent renderer PHASE 0 owns only its run-local proposal.
Validation runner hardening passes 15 mocked tooling regressions. Final checks
require stable source fingerprints; native blueprint scripts must run through
a filtered environment with explicit pass-owned binary and no activation.
Planned final verification is `docs/T3-VERIFICATION.md`, not a passing result.
Renderer PHASE 0 has completed. `docs/T3-SLICE-RENDERER-00.md` authorizes a
20-minute bounded Swift/WebKit isolation feasibility probe in separate renderer
paths, sharing the single build/native slot. No Mermaid acquisition or
production interactive capability is enabled yet.
Expanded tooling checkpoint: validation 17/17 and existing blueprint tooling
95/95, with isolated homes and raw logs in the run root. Frozen contract and
original binary guards pass. Chat F1, engine B1 and renderer R0 are active
independent lanes; do not duplicate workers or bypass their shared build slot.
Checkpoint raw F1 results: app unit 199 passed/21 ignored; serial UI 316 passed.
Acceptance is pending lint and independent review. Initial locked checks hit
the approved dependency-update window; offline engine compilation reported five
missing imports and one unused import. B1 remains under repair, not accepted.
All failure evidence is retained. No next feature slice has started.
Next chat gates are prepared in `docs/T3-SLICE-CHAT-02.md`, but F2a cannot start
before F1 independent acceptance. It adds width and safe Copy/Stage, not provider
execution; F2b actual syntax-highlighting dependencies remain to be coordinated.
Independent F1 review requires cache-owner retirement on successful archive/
disposal, with real selected-parent/child archive/undo coverage. No retirement
may bypass a failed saving/running/task guard. The independent strict app lint
now reaches compilation and fails one unnecessary conversion in the new test.
The same frontend worker is repairing these two findings; F2a is still held.

Renderer R0 finished with five successful compiles, one failed compile, and
34 sandbox launches all aborting before output. There is no raster, executed
negative-security result, or isolation pass. Zero canary hits are inconclusive.
The architect inspected the matching native PID 50432 and Swift PID 50387
crash reports: both fail in the system dynamic loader before `main`, not an
observed WebKit call. The missing loader operation is still unknown.
`docs/T3-SLICE-RENDERER-00D.md` freezes a ten-minute, native-only diagnostic
continuation with narrow loader grants and at most ten launches. The existing
renderer worker may perform it; no duplicate worker, WebKit retry, Mermaid
acquisition, renderer activation, or unsandboxed fallback is authorized.

`docs/T3-SLICE-ENGINE-02.md` prepares the native scheduling/intent boundary only.
B1 independent acceptance and an exact executor/supervisor PHASE 0 proposal
must precede B2 authorization. No provider execution has been enabled by these
prepared gates.

`docs/T3-SLICE-CHAT-02B.md` records the real GPUI grammar requirements and a
new resource concern: the default highlighter retains entries keyed by arbitrary
fence-language strings. F2b needs a fixed-language bounded override across chat
and file-preview paths, plus separately coordinated exact dependency acquisition.
No downloads, feature changes or root-lock edits were made by this preparation.

Primary validation-tooling repair adds renderer C/header/HTML/CSS/module input
hashes. The expanded mocked suite passes 20/20 without Cargo, native launch or
provider execution. A real synthetic HTML mutation during a mocked command is
rejected by the stable-source gate. Raw log:
`evidence/validation-renderer-inputs-1791256041055532000.log`.
Historical source fingerprints are not retroactively upgraded.

R0D PHASE 0 completed without implementation or execution. Its ten disagreements
are ruled on in `docs/T3-RENDERER-R0D-RULINGS.md`: authorize D1–D4 and at most
eight native launches, decline D5/root-directory data visibility. The existing
renderer worker may implement/execute only that finite native matrix under the
shared slot and ten-minute deadline. No full renderer/isolation verdict follows
from this diagnostic, and no WebKit retry is authorized.

F1 repair source checkpoint: owner retirement is after successful archive/
removal and after the existing failed stop/save/task/storage return paths.
Four new real-view lifecycle regressions cover selected parent/children,
overwritten Undo, rehydration, rejected/failed archives and a task-review race.
Their source was reviewed; passing counts and F1 acceptance still await completed
worker checks and independent reruns. No duplicate validation job was started.

B1 worker finished with agent 271 passed/1 ignored and ACP 74 passed, plus
checks/formatting/Clippy exit 0. Three agent additions are concurrent JEV tests,
not B1. Raw report: run-root `specs/backend-b1-results.md`.
Independent review requires two scoped repairs in
`docs/T3-ENGINE-B1-REVIEW-RULINGS.md`: new enum payload boxes instead of warning
allowances, and explicit recursive sorting of version-1 canonical JSON objects.
The original frozen contract baseline stays intact; the additive ruling
corrects only new planned types and does not change legacy serde shapes.
The existing backend worker may repair B1; no B2 execution is authorized.

Additional B1 source finding: pending acceptance objects can validate separately
while their canonical receipt-chain reference is missing or belongs to another
branch. Recovery must reject that state before rewriting history or returning
contradictory queue/receipt projections. The additional frozen gate is
`docs/T3-ENGINE-B1-RECOVERY-REVIEW.md`; actual-store regression evidence remains
pending. Do not interrupt the current backend repair or spawn another worker;
the same worker takes this gate after its current invocation finishes.

F1 repair returned with app units 199 passed/21 ignored and strict app Clippy
exit 0. Its full 17-test Cargo pass was rejected by whole-tree source drift in
renderer files; the stable 15-test rerun excluded two stress tests. Primary
independent full-focused/unit/lint pipeline PID 2596 uses the serial runner,
fresh homes and pass-owned target. No F2a authorization yet.

R0D returned one warning-clean compile, 39 passing mocked tests and three
launches without main output. D2-A's execvp permission failure was overclassified
as a profile error; D3/D4 remain unexecuted. A scoped classifier/continuation
repair in `docs/T3-RENDERER-R0D-CONTINUATION.md` permits only five unused slots,
unchanged grants and cumulative eight-launch/600-second limits. Preserve the
original report/artifacts; no new helper compile, D5, WebKit or renderer
capability is authorized.

Primary F1 pipeline PID 2596 has finished, not left running. Independent full
F1 actual-view module: 17 passed/0 failed; app units: 199 passed/21 ignored;
strict app library/test Clippy: exit 0. All three checks report unchanged
whole-tree source, frozen contracts and original binaries. The repair source/
lifecycle review clears the original findings. `docs/T3-CHAT-F1-ACCEPTANCE.md`
authorizes the same frontend worker to begin the already frozen F2a width/
Copy/Stage slice. No highlighting/renderer claim, final workspace/native
verdict, or duplicate frontend builder follows from this acceptance.

B1 first review repair returned normal/preserve_order focused 28 passed,
agent 273 passed/1 ignored and ACP 74 passed. Strict Clippy still exits 101:
new EngineEvent Progress remains 232 bytes beside the boxed Record. The
additive `docs/T3-ENGINE-B1-FINAL-REPAIR.md` ruling boxes Progress as well and
releases the previously frozen receipt-lineage corruption gate to the same
worker, including stopped-task mutation preflight. Preserve previous reports;
no warning suppression, dependency/root-lock change or B2 execution.

R0D continuation worker completed. Five unused slots all returned launcher
execvp EPERM/exit 71; cumulative native launches are exactly eight. No main
marker or native denial, raster or WebKit isolation evidence exists. Primary
verified 70 new and 55 old artifact paths/hashes, exact raw classifications/
counts/lock-release timing, unchanged helper/templates and preservation bracket,
then independently reran 52 mocked tests through the shared slot (exit 0,
zero native launches). Preflight interpreter/scratch failures are retained.
`docs/T3-RENDERER-R0D-VERDICT.md` records the renderer hold. The installed
dyld-support root-directory open is an unproven hypothesis outside prior grants.
Further launches/permissions need a user decision; no fallback or renderer
enablement. Existing F2a/B1 workers may finish only their current authorized
slices while the recurring checkpoint is paused for this genuine blocker.

The user selected “Approve narrow root-directory diagnostic.” No production
renderer or descendant/home/network grant follows. `docs/T3-SLICE-RENDERER-00E.md`
freezes the separate exact version-1 D1 + root-directory-only candidate,
at most two new native launches/300 seconds after lock, unchanged helper/no
compile, and immediate first-failure stop. The same renderer owner is
PHASE 0 proposal-only until rulings. R0D's eight slots remain consumed.
The prior checkpoint 74615eb6 was cancelled; resumed loop is 3b2e55bc.
B1 final repair returned 37 focused passes in ordinary/preserve_order modes,
agent 282 passed/1 ignored, ACP 74, strict lint and formatting exit 0. Primary
must independently review/validate before B1 acceptance or B2. F2a owner remains
active. No duplicate worker/build or user-app change is authorized.

R0E PHASE 0 dispatched to the same renderer owner; no implementation/native
launch is yet allowed. Its frozen slice SHA is
4971918fdccb0305e5bc85e0231939b8c6f210823729f4a4b388c885646224ba.
B1 primary source review clears the requested layout/sorting/lineage findings.
Independent focused/mode runs each pass 37, full agent 282/1 ignored and ACP 74,
strict all-target agent/ACP lint exit 0. The full run alone recorded unrelated
frontend `chat_code.rs` drift; no whole-tree-stable verdict follows from it.
Finished pipeline PID 52828 is not active. A single stable-source full repeat
is now PID 55994, log
`/var/folders/m_/lmx0wysj2p122001hkcqnw3r0000gn/T/droid-bg-1791259357969.out`.
B1 acceptance/B2 remain held until the repeat is reviewed. Review checkpoint:
run-root `specs/backend-b1-review.md`.

Stable-source B1 repeat PID 55994 has finished: Cargo exits 0 with agent 282/
1 ignored and ACP 74, but runner exit 3 rejects the source-drift gate because
frontend `tests/blueprint_ui/chat_readability.rs` was added during the check.
Agent/ACP source and frozen/binary guards are unchanged. Do not start another
repeat until F2a source settles; do not weaken the final stable-source gate or
claim B1 acceptance/B2 permission from either drifted full run. Existing
frontend/R0E proposal workers continue within their scopes.

R0E proposal returned twelve substantive disagreements with installed DIRECTORY
selector evidence and the exact 1014-byte candidate hash. All are ruled on in
`docs/T3-RENDERER-R0E-RULINGS.md`: implement three new files only, conservative
nonblocking/first-failure stop, actual foreground controller exit and one-write
inventory. No test/native/control launch until primary source review and a
separate external seal at run-root evidence/renderer-r0e-execution-seal.json.
The same renderer owner returns `specs/renderer-r0e-implementation.md` before
execution. User root visibility is diagnostic-only; no renderer capability.

Preservation recheck verified all 235 source-before files/3,249,894 bytes plus
current frozen-contract/original-binary guards; receipt
`evidence/checkpoint-preservation-1791259800666763000.json`. R0E implementation
dispatched to the same owner, three new files/static inspection only.
`docs/T3-SLICE-ENGINE-03.md` prepares bounded critical-record/progress delivery
after B1/B2, not code authorization. It records exact native/child/ACP
unbounded/try_send paths and EventWriter retry/barrier hazards, root-wide
limits, stale-generation and append/replace semantics, independent controls
and required durable spool/cursor evidence. No next backend slice started.

Latest recurring checkpoint read the plan/todos and checked live ownership
without starting a job. Existing frontend owner is still running and preparing
`specs/frontend-f2a-results.md` with a ruling request; await its completed raw
report before resuming or changing scope. Existing R0E owner is still typing
only its three new source files, no test/native execution seal. Build/native
slot was idle, no Cargo/rustc/native probe observed; frozen-contract and
original-binary guards pass. B1 stays held for settled-source full acceptance.
Do not duplicate/resume an owner based on this status peek.

F2a invocation finished with four actual-view passes/one retained-selection
failure. All compile/runtime failures are retained; full regressions/format/
strict app lint remain pending. Primary confirmed the native Capture clear,
touch-only preservation and capture-stop/Bubble cancellation in installed
GPUI Base/Pre. `docs/T3-CHAT-F2A-SELECTION-REVIEW.md` keeps the frozen gate and
requests same-owner proposal-only for a bounded opt-in native preserving-control
API, potentially repository-owned locked GPUI Base with explicit offline root
dependency coordination. No registry/toolkit/root/dependency edit is yet allowed,
no fake event/range restore or global policy workaround, and no next app slice.
R0E's existing owner continues its separate source-only scope.

R0E implementation-only worker finished: three new sources, two valid Python
ASTs and 57 unrun mocked tests; no import/test/compile/native launch or seal.
Primary read the complete sources and independently checked the 1014-byte
profile delta, source hashes and unchanged original helper identity. Static
review found no correction needed before the finite experiment; run-root
`specs/renderer-r0e-primary-source-review.md` records the inspection and limits.
The external seal is still absent. Independently verify the exact preservation
closure before issuing separate execution authorization; never infer a native
or renderer result from this source review.

Paused F2a action review is in `T3-CHAT-F2A-ACTION-REVIEW.md`: list-nested fence
continuation is reconstructed with the wrong prefix, unchanged hook renders
deep-copy retained source spans, and hidden-owner deferred Copy lacks the
stale-owner coverage/visibility guard that Stage already has. These corrections
queue for the same frontend owner after its still-running proposal-only native
selection invocation. No duplicate app edit, toolkit/root change or build was
started. Fresh frozen-contract/original-binary guards pass; shared slot was idle.

Primary B1 settled-source full repeat completed: wrapper/Cargo exit 0, agent
282 passed/1 ignored, ACP 74 passed, unchanged whole-source/contracts/original
binaries. Earlier independent focused/mode runs each pass 37 and strict lint
exits 0. `T3-ENGINE-B1-ACCEPTANCE.md` accepts the durable foundation for bounded
progression and authorizes only same-owner B2 PHASE 0, not execution or code.
Full evidence: `evidence/b1-independent-settled-full-1791261721874231000-result.json`.

Primary R0E preservation verification passed 790 exact closure hashes, 55 old/
70 continuation artifacts, 702 old guard entries and immutable raw identities,
plus all 235 starting snapshot files/3,249,894 bytes. No runner import/test/native
launch. `T3-RENDERER-R0E-EXECUTION.md` permits creation of the fixed primary-owned
external seal and one foreground controller invocation under unchanged finite
limits. It does not allow retries, further grants or renderer integration.

The sealed R0E controller has finished. Actual foreground exit 0; 57 mocked
tests pass; exactly E-A/E-B reach protocol-1 main and exit 0. Both synthetic
file-open probes and actual loopback connect calls return EPERM, with healthy
closed canaries/zero hits and unchanged fixtures. Independent review verifies
33 indexed payloads/35 total artifacts, all 790 preservation hashes, exact
44 cumulative consumed launches and actual lock release/final accounting;
review adds zero launches. `T3-RENDERER-R0E-VERDICT.md` accepts only this native
diagnostic. Full renderer capability remains held. Same-owner raw reporting/
WebKit follow-up PHASE 0 is allowed; no permission carry-over or new execution.

B2 PHASE 0 has been dispatched to the same completed backend owner, proposal
only. Frontend native-selection PHASE 0 remains active with action-review
corrections queued. No production builder or Cargo/native job is now running.

The same renderer owner was resumed for two new run-root documents only:
raw R0E results and WebKit feasibility PHASE 0. It may not execute, compile,
edit source or carry the diagnostic root exception into WebKit. Backend B2 and
frontend native-selection owners are likewise proposal-only. Await their actual
completion reports, not stale status listings. Primary B1 review note is updated
with the final stable acceptance. No further native slots or overlapping builds
were started; 3b2e55bc remains the useful-work checkpoint.

The later recurring request's B1/R0E wording is historical, not a rollback of
the newer acceptance/execution rulings above. Current checkpoint reread this
handoff/implementation plan and existing report directory: native-selection,
B2 and WebKit PHASE 0 reports have not returned yet. Fresh frozen-contract/
original-binary guards pass and shared slot is idle. No duplicate worker,
completed-delivery poll, build, native retry or permission change was started.
Await the existing owners' actual reports; keep 3b2e55bc active while useful
authorized proposal work remains.

Primary continued the actionable paused-source verification rather than
starting a duplicate builder: inspected Stage's ordinary input/Change routing,
the isolated fixture home/workspace and existing inode-preservation test pattern.
`T3-F2A-VALIDATION-NOTES.md` prepares complete bounded fixture-tree nonmutation
evidence, without claiming a filesystem/test pass. Pane-width routing in
`docking.rs` matches preserved dirty source and is not an omitted F2a change.
Verification closeout now identifies active loop 3b2e55bc instead of the already
cancelled old loop. One nonblocking frontend progress peek confirms its proposal
is still running; it was not woken, resumed, duplicated or graded. Remaining
primary implementation/ruling work depends on the actual proposal reports.

Primary ran previously pending F2a checks while all owners were proposal-only:
app units 201 passed/21 ignored and strict app library/test Clippy exit 0.
Both wrappers report unchanged source/contracts/original binaries; evidence
`f2a-primary-paused-units-1791262784796131000` and
`f2a-primary-paused-clippy-1791262796394090000`. This is not F2a acceptance;
the frozen real-view selection/action/formatting gates remain incomplete.

B2 PHASE 0 returned 12 substantive disagreements and bounded B2a/b/c seams.
`T3-ENGINE-B2-RULINGS.md` chooses B2a private native execution/checkpoint/intent
foundation and rules on all twelve. Independent API inspection identified
reqwest 0.13.5's transport-level retries/redirects, requiring explicit never/
none/no-proxy native-client policies rather than just removing Provider retries.
The same backend owner may prepare the exact schema/interfaces/transition/
fault contract only before primary freeze. No B2 implementation/execution.

B2a exact-contract proposal was dispatched to the same backend owner under the
new frozen rulings; no code/build/provider/tool execution. Renderer returned
raw R0E results plus a 15-disagreement WebKit W1 proposal, with no new compile/
test/native launch. `T3-RENDERER-W1-REVIEW.md` rules on all fifteen and holds
W1 implementation/execution: public WK does not supply positive all-service
ownership/teardown or hard aggregate memory/no-prompt containment. No root-rule
carry-over, framework/Mach/service grant or unsafe fallback is approved.
This is a genuine renderer architecture input blocker; choose bounded
proposal-only disposable-containment research or hold rendering while current
chat/engine slices finish. Existing frontend/B2a proposals remain authorized.

Checkpoint loop 3b2e55bc is now cancelled at this genuine unresolved renderer
input blocker, not at full implementation completion. Primary froze the W1
review/proposal/raw-report hashes; all frozen-contract/original-binary guards
and tracked whitespace checks pass. Request explicit bounded proposal-only
containment research versus renderer hold before resuming that track.
Frontend native-selection and backend B2a exact-contract invocations may finish
their existing proposal-only scopes; no worker was cancelled or duplicated.

The user subsequently selected **Research disposable containment**.
`T3-RENDERER-CONTAINMENT-RESEARCH.md` freezes same-renderer-owner research only:
one new proposal, no environment creation/acquisition/mounts/network probes,
source implementation, compile/test/native launch, new grants or seal reuse.
Read-only published API research is permitted, not renderer network access.
All 44 historical native launches remain consumed; W1 stays held.

Frontend native-selection and backend exact-contract invocations have now
completed. Their new reports are run-root
`specs/frontend-f2a-native-phase0.md` (six disagreements) and
`specs/backend-b2a-contract-proposal.md` (three stated limitations plus exact
private schema). Both await independent architect rulings; completion is not
implementation/copy/root-lock/activation authority. Resume only the existing
owners after the applicable frozen scope. The pass-owned build slot was idle
at this checkpoint and user-app PID 46930 remains untouched.

The renderer research-only invocation has been resumed with its existing owner.
Useful-work checkpoint is now `544a56d2` (15 minutes, no immediate fire); both
older loops remain cancelled. Do not replay exhausted native diagnostics.

Frontend D1–D6 are ruled on in `T3-CHAT-F2A-NATIVE-RULINGS.md`. N0 authorizes
only verified Base/Pre copies, relative exclusions/path patches, source/checksum-
only lock reconciliation, full-copy fingerprint regressions and uncapped strict
native/production baselines. No selection API/action/app edit yet. Primary also
requires separating valid visible tiled Copy from active-pane selection
exemptions, actual computed-style visibility and native focus behavior before
N1. Stop on any lock/dev-graph/lint-cap/upstream-warning prerequisite.

The same frontend owner has been resumed for N0 only. Renderer containment
research remains separate and proposal-only; no second source writer/build
slot or native launch has been introduced.

Primary read the complete B2a exact proposal and independently inspected its
actual store/persistence/task/Session/approval seams. The three stated limits
are accepted only as scoped truthful limits. `T3-ENGINE-B2A-CONTRACT-REVIEW.md`
returns four corrections: equal-revision transaction predecessors, exact
v1/v2/stopped-task queue accounting, two-pass zero-publication preflight, and
a smaller test-only immutable-blob-codec first stage. Same backend owner may
write one new correction contract only; no code/build/activation yet.

Backend correction-contract work has been resumed with the existing owner.
Current independent invocations: frontend N0 provenance/baseline (sole source/
root/tooling owner), backend correction proposal only, renderer containment
research only. Primary's new freezes verified 27 prior frozen identities and
both original release binaries; tracked whitespace check exited 0. No primary
Cargo/native validation or new native launch was added in this review interval.
Await actual completed reports before grading/resuming any running invocation.

Primary's later read-only preservation checkpoint independently verifies all
235 starting snapshot files/3,249,894 bytes unchanged; receipt
`evidence/checkpoint-starting-snapshot-1791291302922847000.json`. Final
verification now names checkpoint 544a56d2 and includes explicit uncapped
native library lint/full-copy provenance if the N0/N1 path copies are retained.
This preparation starts no additional build, test or native job and records
no new feature pass. Current pending progression depends on the three owners'
actual completion reports, not another duplicate invocation.

Renderer containment research has actually completed. Primary read the full
709-line report and ruled on all twelve disagreements in
`T3-RENDERER-CONTAINMENT-VERDICT.md`. The bounded shared-Mac research identifies
no satisfying design: Virtualization.framework guest RAM/cgroups do not provide
hard aggregate host limits, and Starting/owner-death/all-service bounded
teardown remains undocumented. No VM/build/launch or capability result exists.
All 44 historical native launches stay consumed and W1 stays held.

The new genuine renderer input decision is a separately bounded dedicated-
Linux-host research proposal versus holding renderer while chat/engine
progresses. No different-platform scope or environmental effect is yet
authorized. Frontend N0 and backend correction proposal have not returned
actual completion reports; their existing invocations may finish without
duplication/interruption. The shared build slot was idle at this checkpoint,
and user-app PID 46930 remains untouched.

Checkpoint 544a56d2 is now cancelled at the genuine new containment input
blocker, not full implementation completion. The research report/verdict and
historical renderer report identities are frozen; all frozen contracts and
original binaries verified unchanged, tracked whitespace exit 0. Ask for
dedicated-Linux proposal-only research versus renderer hold before resuming
that track. Do not cancel or duplicate current frontend/backend invocations.

The user selected **Hold renderer; continue chat/engine**.
`T3-RENDERER-HOLD.md` records the deliberate hold, not a waiver of the full
renderer target. Renderer owner is idle; no dedicated-Linux research or
environmental effect is authorized. Resume useful chat/engine progression
without repeatedly reopening this held track.

Frontend N0 has actually completed and stopped at the strict-native diagnostic
prerequisite: 31 mocked tooling passes and production check exit 0; Base Clippy
exit 0 was Fresh with zero selected driver invocations, not uncapped native
lint proof. Pre Clippy/N1 were not run. Both locked native source copies,
minimal root patches/lock delta and full-copy tooling exist; all 363 original
packaged bytes and prior F2a source/failure evidence are reported unchanged.
Primary must review artifacts and freeze the narrow diagnostic follow-up.
Backend correction-proposal invocation has not returned completion yet.

Renderer hold is frozen in `contracts-frozen-renderer-user-hold.json`.
Useful chat/engine checkpoint is now `1618659f`, fifteen minutes without an
immediate fire. All older loops remain cancelled. Renderer is deliberately
idle; do not reinterpret this checkpoint as renewed renderer permission.

Primary independently reran all 31 N0 mocked tooling tests, exit 0, under the
shared slot with unchanged whole-source/tooling/frozen-contract/binary guards.
Archive extraction inventories, all 363 original files/modes, both provenance
files, exact root deltas/917 packages and both actual production compiler
identities are verified in
`evidence/f2n-n0-primary-provenance-1791292515753686000.json`.
Two primary verifier mistakes were corrected and retained; neither changed
source or added a Cargo/native/provider launch. Base Fresh-only Clippy still
provides no native lint proof, and Pre remains unrun.

`T3-NATIVE-LINT-PHASE0.md` accepts only mechanical N0 progression and assigns
one same-owner proposal for a closed, identity-selected actual-driver
diagnostic. No tooling implementation/test/real check is yet authorized.
Native library identities are Base `gpui_base`/`src/lib.rs` and Pre
`gpui`/`src/gpui.rs`; package-name guesses are insufficient. Root/exclusions/
lock, upstream source and all N1/F2a gates stay unchanged. The existing backend
correction invocation is still awaiting its actual completion report.

The frontend owner was subsequently resumed for the one proposal only. Backend
corrections then actually completed: R1–R4 and five new constraints are recorded
in `specs/backend-b2a-contract-corrections.md`, with no implementation or tests.
Primary read the complete 903-line report and relevant actual storage/lease/
registration seams. No B2a-0 source authority has been issued.

The user now requests **just finish this up, do not continue the loop**.
Checkpoint 1618659f has been cancelled. `T3-CLOSEOUT.md` replaces pending
continuation with bounded serial current-source checks and honest stopped
handoff. Do not restart a loop or authorize another feature stage. The already
running frontend proposal can finish without source/test/launch authority.
Full scope, F2a selection/actions, uncapped native lint, B2 execution and the
deliberately held renderer remain incomplete, not waived.

Closeout checks have completed exactly once, serially, in 606.804 seconds.
Formatting exits 1; workspace/all-target strict Clippy exits 0. Workspace tests
exit 101: ACP 74 pass, agent units 282 pass/1 ignored, app units 201 pass/
21 ignored, actual UI 323 pass/2 fail. Failures are the known width-selection
assertion and stopped-task-scope retry error assertion. B1 preserve_order
37/37 passes. All source/tooling/frozen-record/file/proposal/original-binary
guards stay unchanged; 235 starting backups/3,249,894 bytes are verified.
Controller: `evidence/closeout-controller-1791293033635882000-result.json`.

Frontend native-lint PHASE 0 has now actually completed: one 360-line proposal,
eight disagreements, zero tests/compiler/native launches. Primary read it
fully. Both latest frontend/backend proposals are retained and held, not
implementation authority. All three owners are idle and `Loop list` returns
no scheduled loops. Do not resume them or restart the checkpoint on this
stopped pass. Full outcomes/dispositions/unrun gates:
`T3-CLOSEOUT-RESULTS.md`. This closeout is not green full-scope completion.
