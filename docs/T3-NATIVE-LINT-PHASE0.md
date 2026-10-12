# Native lint diagnostic follow-up, proposal only

## Authority and current evidence

The user holds renderer and continues chat/engine. This follow-up concerns only
the missing N0 native-library lint prerequisite. It does not authorize N1,
native selection/app/action changes, renderer work or a GUI/probe launch.

Primary independently verified the cached archives, all 363 original copied
files and modes, the 365-file final inventories, exact root manifest/lock byte
deltas and all 917 retained packages. Nonowned source, frozen contracts and
original binaries are unchanged. Production check evidence selects both local
libraries. All 31 mocked tooling tests passed independently under the shared
slot. Receipt: run-root
`evidence/f2n-n0-primary-provenance-1791292515753686000.json`.

The primary's two verifier errors are retained separately: incorrect patch
table placement, then a guessed Pre library name. They changed no repository
file and added no Cargo/native/provider launch. They are not native failures.

N0 mechanical provenance/tooling is accepted for diagnostic progression only.
N0's mandatory native lint gate is still unmet. Base Clippy exited 0 with no
selected compiler/driver invocation; Pre Clippy was not run. Neither passing
lint nor upstream warning failure has been established.

## Verified target identities

| Package | Version | Library crate | Production source |
|---|---|---|---|
| gpui-base | 0.7.0 | `gpui_base` | `crates/flint-native-gpui-base/src/lib.rs` |
| gpui-pre | 0.3.7 | `gpui` | `crates/flint-native-gpui-pre/src/gpui.rs` |

The production raw log has one actual rustc library invocation for each, with
no `--cap-lints`. Do not infer Pre's library name or entry point from its
package name.

Installed Rust 1.95.0 Cargo documentation states that
`RUSTC_WORKSPACE_WRAPPER` applies only to workspace members, while
`RUSTC_WRAPPER` wraps ordinary rustc invocations. Installed Clippy usage
documents direct `clippy-driver` use with rustc arguments. Both native packages
are intentionally excluded from workspace membership. This supports a
selection/wrapper hypothesis, not a proven single cause of the Fresh result.

## One bounded PHASE 0 assignment

Only the existing frontend owner
`da6c5976-0e43-4814-b8dc-9e746b980e45` may write the new run-root
`specs/frontend-native-lint-phase0.md`. Read the current authority and native
rulings, inspect real installed APIs/formats read-only, and return substantive
numbered disagreements before any implementation. No tests or commands that
invoke Cargo, rustc, Clippy, native probes or the application in this stage.
No network lookup/acquisition, environment dump or private config/credential
inspection. Do not edit any existing source, tooling, contract or report.

The selected implementation direction for the proposal is a private,
closed-purpose run-owned Cargo/rustc wrapper controller, not new workspace
membership, standalone package resolution or graph changes:

1. Plan two sequential offline locked library checks from the existing root
   manifest, Base then Pre. Each gets its own fresh, initially empty run-owned
   target and private HOME/FLINT_HOME. No cache deletion or seeded selected
   artifacts. The shared build lock, filtered environment, whole-source,
   frozen-contract and original-binary guards remain mandatory.
2. Explicitly controlled `RUSTC_WRAPPER` selects only the requested package's
   exact manifest directory, package/version, library crate, source and `lib`
   target. All other trusted compiler invocations pass through unchanged to
   the pinned real rustc. No global Clippy policy or inherited arbitrary
   wrapper/flags. Account for Cargo compiler-information queries.
3. The selected library must actually execute the pinned matching
   `clippy-driver` with its complete original compiler arguments and
   `-D warnings`. Reject a selected command-line cap, new allowance or wrapper
   suppression; do not overwrite a cap to make the run appear uncapped.
   Inventory existing unchanged upstream lint attributes as provenance,
   not a license to add allowances. No dev/test/bench graph, feature changes,
   source repair or new warning allowance.
4. Retain bounded structured invocation/exit records proving selection,
   executable/source identity, actual driver execution and lack of a
   selected diagnostic cap. Zero selected invocations, Fresh-only output,
   ordinary rustc-only output, missing evidence or a failing driver is not
   a strict lint pass. Ordinary dependency logs are not selected proof.
5. Propose a minimal independent controller/wrapper and mocked test boundary.
   Keep `specs/validate.py` and its general command/environment authority
   unchanged unless a specific unavoidable lease is returned for review.
   Cover wrong identities, unexpected flags/caps, queries/dependencies,
   zero/multiple selected invocations, wrapper/tool drift, lock contention,
   source drift, nonzero exits, deadlines and owned-group cleanup.
6. Freeze executable/tooling hashes and exact commands before real execution.
   Propose no more than two Cargo invocations, at most 1200 seconds each and
   2700 seconds total, stopping after the first failed prerequisite. First
   implement and independently validate mocked-only tooling, then obtain
   separate primary execution authority. No automatic retries.

Return exact argv/environment additions, file ownership, guard coverage,
installed tool identities, resource/deadline/cleanup semantics and any
limitations that prevent this plan from proving real uncapped diagnostics.
Do not silently substitute a different approach or claim feasibility from
documentation alone.

## Unchanged stop conditions

Stop and report if the existing locked production graph cannot support this
selection or a required API/driver behavior is unverified. Do not drop
`--locked`, use a packaged nested lock as authority, add members/dependencies,
run a standalone native manifest, modify original upstream files or waive
strict diagnostics. Original N0/F2a failure evidence and all renderer history
remain immutable. N1 requires actual diagnostic results and another freeze.
