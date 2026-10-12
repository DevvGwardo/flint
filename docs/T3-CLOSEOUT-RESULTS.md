# Stopped T3 pass: actual closeout results

## Outcome

The recurring loop is stopped at the user's request. `Loop list` returned
**No scheduled loops**. All three existing owners have completed their latest
bounded assignments and are idle. No next implementation stage is authorized.

This is a stopped partial implementation, **not a green full-scope completion**.
F1 retained Markdown/resource safety and B1 durable acceptance/recovery retain
their bounded independent acceptances. Public B1 remains nonexecuting.
F2a selection/actions, native strict lint, B2 execution and other planned
chat/engine features remain unfinished. Full isolated HTML/Mermaid is still
deliberately user-held.

## Fresh serialized checks

Evidence root:
`/Volumes/T7 Shield/mac-offload/flint-t3-implementation-glct94hg/evidence/`.
One controller ran the four frozen closeout commands exactly once, offline
where applicable, with fresh private homes, one shared build slot and stable
whole-source guards. Total elapsed: 606.804 seconds. No timeout or drift.

| Check | Actual outcome | Evidence stem |
|---|---|---|
| Workspace formatting | exit 1; formatting differences in app/chat source and UI tests | `closeout-format-1791293033731500000` |
| Workspace/all-target Clippy, `-D warnings` | exit 0 | `closeout-workspace-lint-1791293034731351000` |
| Workspace tests, serial | exit 101; UI failures below | `closeout-workspace-tests-1791293052127945000` |
| B1 preserve_order, serial | 37 passed, 0 failed, 0 ignored; exit 0 | `closeout-b1-order-1791293625805812000` |

Workspace test suite results:

- ACP: 74 passed, 0 failed.
- Agent units: 282 passed, 0 failed, 1 ignored.
- App units: 201 passed, 0 failed, 21 ignored.
- Actual UI: 323 passed, 2 failed, 0 ignored.
- Evaluation's four offline checker tests passed. Its opt-in live evaluation
  body was disabled. Three benchmark entries and the live-provider entry
  returned without executing their opt-in bodies because the isolated
  environment sets `FLINT_BENCH`, `FLINT_EVAL` and `FLINT_LIVE` to `0`.
  Their harness “ok” statuses are not benchmark/live-provider evidence.
- Later terminal/doctest suites were not reached after the UI suite failed.

Failed UI tests:

1. `chat_readability_ui::chat_width_settings_real_control_persists_and_reflows_without_reset`:
   `chat_readability.rs:74`, selected range was `None`, expected `Some(0..8)`.
   This reproduces the known unresolved F2a Capture selection gate.
2. `task_outcomes_ui::review_boundaries::failed_stopped_scope_save_keeps_edits_for_a_retry`:
   `task_review_boundaries.rs:504`, expected `task-sheet-error` was absent.
   This fresh full-suite failure is retained and not repaired or attributed to
   a baseline/flake without evidence. Its fixture creates `history.json.tmp`
   before saving; current leased immutable storage uses its own anchored
   temporary names. Further diagnosis/test repair was not authorized in this
   user-requested closeout.

Formatting failure and both UI failures remain open. No assertion change,
source repair or retry was performed. Strict workspace lint is not uncapped
lint proof for excluded native libraries.

Controller receipt:
`closeout-controller-1791293033635882000-result.json`.
All source, tooling, frozen-record/file/proposal and original-binary guards
are true. It also verifies 235 starting backups / 3,249,894 bytes. The aggregate
controller exited 1 because checks failed, not because provenance failed.

## Proposal-only outcomes and dispositions

Backend `specs/backend-b2a-contract-corrections.md` completed, 903 lines,
SHA-256 `e3359d6870d9dd5a4e17f4755920b0fff4d12706a536f5184ad1246b13751280`.
Primary read it and the actual Directory/WriterLease/Store/registration seams.
No B2a-0 module, v2 activation, stopped-task rewrite or executor was implemented.

New backend disagreements:

1. Deterministic owned sources: accept the limitation of generic Serde; no
   codec implementation authorization.
2. Immutable reuse allocations: accept the identified hidden allocation and
   truthful cooperative-lease scope; proposed instrumentation/bounds are
   unrun and unapproved as actual evidence.
3. Provisional streaming callbacks: accept that final verification cannot
   retroactively certify early callbacks; no authority consumer is approved.
4. Task-revision eligibility fences: accept the required distinction from a
   boolean gate; persistence implementation remains held.
5. Start/Prepare content-address cycle: accept the identified dependency-cycle
   correction in principle; the full future schema still needs separate
   validation/freeze and is not execution authority.

Frontend `specs/frontend-native-lint-phase0.md` completed, 360 lines, with
zero tests/compiler/native launches and no existing source/tooling changes.
Primary read it completely. It proposes three private scripts, not code.

New frontend disagreements:

1. Documentation versus execution proof: accept the limitation.
2. Explicit empty Clippy config versus parent search: acknowledge the proposed
   prerequisite; file/environment creation authority is held.
3. Existing upstream lint policy: accept preservation/inventory without new
   allowances; no diagnostic result has been established.
4. Query/build-script classification: accept the distinction; proposed selector
   behavior is unimplemented and untested.
5. Private diagnostic lease: no general-validator change is approved; the
   new private tooling lease is held by this closeout.
6. Actual driver/evidence requirement: accept that argv/Fresh/exit 0 alone is
   insufficient; proposed records are not executed evidence.
7. Watchdog/owner-death/resource limits: retain stated limitations; no watchdog,
   containment guarantee or execution authority is approved.
8. Cold-build budget uncertainty: accept the stop/no-retry requirement; no cold
   diagnostic was executed.

Both proposals are retained for possible later user-directed work. Their
implementation and execution stages are not approved by this document.

## Preserved and unrun

- Original dirty/untracked source and historical failure evidence are retained.
- All 363 original native packaged files are unchanged; 365 final copied files
  including provenance are fingerprinted. Independent tooling tests: 31 passed.
- Original release binaries are unchanged. The running user app was not
  restarted, replaced or activated.
- No commit, push, publication, credential use, acquisition, renderer retry,
  permission change or release/native GUI promotion occurred.
- Full native library lint, release build, native screenshot inspection,
  physical input, VoiceOver and live providers remain unrun/unverified.
- Historical 44 renderer native launches remain exhausted. Renderer stays
  held, and no checkpoint loop is scheduled.
