# Evidence-driven native functionality/orchestration loop

Use this prompt with an available Factory worker (or a bounded set of workers with non-overlapping file ownership). Do not assume Codex, Cursor, an external agent CLI or a scheduler is available.

## Assignment

Analyze Flint's actual code and callers, then enact small, high-confidence functionality/orchestration corrections. Focus on prompt intake and steering, child coordination/cancellation, stale verification evidence, retries, completion nudges, queue holds, shutdown and durable recovery. Read current TASK-OUTCOMES and ORCHESTRATOR-EVAL documentation, applicable repository instructions and the explicit current authorization. Historical stopped-loop/handoff gates are context, not permission for major orchestration implementation.

Execution ending is never user acceptance. Preserve approval, review, read-only, evidence freshness and queue-hold protections. Do not weaken assertions, certify semantic completion from a file/check/child transport success, or auto-accept a result.

## Safety and baseline

1. Establish the repository and inspect its current changes. Preserve all existing dirty/untracked work. Take a source-before snapshot and SHA256 manifest, with separately hashed protected application binaries. Never reset, restore or clean user changes. Refuse to overwrite any existing prompt or evidence artifact; use a fresh run directory under target.
2. Isolate HOME, TMPDIR and CARGO_TARGET_DIR under the run directory. Use the existing public Rust toolchain/cache via explicit RUSTUP_HOME/CARGO_HOME; never change user HOME state. Set FLINT_LIVE=0 and FLINT_EVAL=0. Never use real providers, credentials or account routing. Scripted loopback test servers are permitted.
3. Before implementation run `cargo test --offline --locked -p flint-agent --lib`, saving merged raw output, the exact command/environment and immediate exit status. Investigate any baseline failure before choosing a fix; retain the failure and original log. Never skip or relax sandbox assertions to make the suite pass.
4. Do not replace target/release/{hooter,flint} or target/debug/{hooter,flint}; build only in the isolated target. Do not launch or restart the real app. No renderer/UI/ACP/platform work, Cargo dependency changes, commits, pushes or publication unless separately authorized.

## One bounded cycle at a time

1. Trace actual callers and authoritative state. Select one concrete small bug supported by code and observable behavior, not a speculative redesign.
2. Add a deterministic regression that exercises the bug and relevant safety boundary. Run it against the unchanged implementation, preserving the actual failing-before log/status. A compilation error, timeout due to a broken test fixture, or an unrelated failure is not a bug reproduction; fix the fixture and rerun.
3. Make the smallest localized fix. Re-read files fresh before editing and preserve unrelated current edits. Run the focused regression and neighboring tests. Check lint/format for the intended changes; preserve failing attempts instead of overwriting logs.
4. Integrate failure/recovery review: interrupted parent/children, denied approvals, queued read-only restrictions, current versus stale checks, no-progress/budget stops, queue holds, restored state and orderly shutdown must remain truthful. Add narrowly scoped companion coverage where a fix could weaken these protections.
5. Maintain a run/native-cycles.md ledger with caller/root cause, semantic impact, files changed, regression name, exact commands/environment, exit statuses, actual counts, raw evidence paths, limits and deferred larger issues.
6. Repeat only while another small high-confidence supported fix remains. Stop at a safety/resource/human decision boundary or when this bounded audit finds none. Report concrete gaps instead of expanding scope. No endless scheduler, periodic background loop or main-thread wait on one.

## Closeout

Run the full offline/locked flint-agent library suite and `cargo clippy --offline --locked -p flint-agent --all-targets -- -D warnings` in isolation. Run rustfmt check only for the Rust files this worker touched. Report genuine failures and ignored/skipped tests, including unavailable platform/runtime coverage; never claim unexecuted checks passed.

Compare source-after fingerprints with source-before. Record the precise mutation closure, including concurrent workers' separately owned edits, and confirm the four protected binaries are unchanged. Retain original snapshots/logs. Return exact edited paths, bugs and semantic effects, failing-before/passing-after evidence, final counts/statuses, source hashes/mutation closure, raw evidence paths and deferred work. Independent acceptance belongs to the reviewing human/parent, not this loop.
