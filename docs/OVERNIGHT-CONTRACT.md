# Overnight loop: frozen safety and validation contract

Version: 1. Frozen before overnight results exist.

## Boundaries

1. Default maximum wall time: eight hours. Default final regression reserve:
   45 minutes. Do not start a builder whose timeout would invade that reserve.
2. One supervisor per source repository; one builder/reviewer/validation
   subprocess at a time. Native blueprint scripts never overlap each other
   or compilation/test measurement phases.
3. Snapshot the current tracked and relevant untracked working tree into a
   fresh run directory. Exclude `.git`, generated output, caches, credentials
   and user data. Refuse source symlinks or unsafe paths rather than follow
   them outside the checkout. Keep original bytes and SHA-256 fingerprints.
4. Never edit application sources in the original checkout. All candidate
   changes are in disposable copies, retained rather than deleted on failure.
   No Git commit, reset, clean, push, merge or automatic patch promotion.
5. The original source fingerprint and original release executable must remain
   unchanged. If another actor changes them, stop with a drift finding; do not
   overwrite or revert their changes.
6. Tests use a fresh HOME/FLINT_HOME below a run-owned `target/`, outside
   globally writable `/tmp`, with explicit CARGO_HOME/RUSTUP_HOME.
   Remove credential/provider variables and automation focus overrides.
   Set `FLINT_LIVE=0`, `FLINT_EVAL=0`, `FLINT_BENCH=0`.
7. Native captures, if explicitly enabled, get fresh output roots and
   `FLINT_BP_NO_ACTIVATE=1`; headless Cargo tests must not get that flag.
   Never activate, restart, close or otherwise drive the existing user app.
   Default native checks are skipped with a reason, not represented as passed.
8. Builder uses existing Cursor authentication as intended, with sandbox
   explicitly enabled. No MCP auto-approval. Reviewer is independent.
   Missing authentication, unsupported sandbox or unavailable reviewer blocks
   the run; do not silently bypass controls or switch to a paid model.
9. Fixed subprocess timeouts, monotonic deadline, stop marker and SIGTERM/SIGINT
   handling. Terminate and reap only run-owned process groups. Never cancel
   unrelated work. Keep timeout/interruption evidence.
10. Stop after two consecutive failed/unproductive candidates, missing review,
    contract mutation, out-of-scope edits, low disk space, source drift or
    deadline. No cleanup of pre-existing files.

## Per-candidate engineering gates

- PHASE 0: builder documents its plan and real disagreements, citing source.
  The architect/human must resolve substantive scope changes before promotion.
- One bounded hypothesis. At most eight application source/test paths changed
  per candidate; explicitly permitted evidence files do not count toward eight.
- No dependency, lockfile, permission, public protocol/schema or benchmark
  threshold changes. Cross-cutting queue delivery changes are investigation
  only until a separate contract is reviewed.
- Add a focused regression or differential oracle. Measure a relevant release
  fixture when claiming a performance improvement. Retain raw samples, fixture
  description, sample count, units, source/binary hashes and limitations.
- Do not replace a failing comparison with a favorable rerun, relax thresholds,
  suppress lints, delete tests, or infer speedups from budget passes.
- Every candidate gets these independently executed gates, in order:

```sh
cargo fmt --all -- --check
cargo clippy --offline --locked --workspace --all-targets -- -D warnings
cargo test --offline --locked --workspace -- --test-threads=1
cargo build --offline --locked --release -p flint-app
```

- Full workspace tests include the serial blueprint UI integration target.
  A separate repeated UI run is optional additional evidence, not required
  twice per candidate.
- Release probes are opt-in, offline and serial. Their exit status is smoke
  evidence unless a proper retained before/after comparison exists.
- Independent reviewer must return `APPROVE` with no unresolved defects.
  Supervisor checks the actual exit status, scoped diff, immutable contract,
  source stability through review and validation, and all gate outcomes.
- The strongest automated state is `proposed-validated`, not merged, accepted
  by the architect/human, visually approved or production-proven.
- Any later candidate based on a previous proposal is a cumulative experiment.
  The morning report must distinguish the per-cycle delta from the cumulative
  diff and re-run final integration gates for the retained stack.

## Blueprint oversight contract

Retain a machine-readable run manifest, append-only event ledger, atomic
status JSON and generated Markdown review report in the run directory.

For each hypothesis record: stable ID, area, priority, evidence anchors,
starting source fingerprint, proposed changes, changed paths and line counts,
baseline/after commands and raw artifacts, test/review exits, measured scope,
remaining risks, verdict, and follow-up questions created by that change.

Track the loop's own overhead: wall time versus active subprocess time,
phase duration, attempted/validated/rejected/no-change counts, source/test
delta, evidence bytes and reasons for skips. Unknown metrics stay unknown.
Do not invent allocation counts, costs, active work or numerical debt scores.

History in `STATE.md`, `GATES.md` and `docs/IMPROVEMENT-LOOP.md` is immutable
input for this run, not a report to rewrite with retrospective claims.
