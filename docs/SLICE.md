# Slice: bounded overnight supervisor and blueprint oversight

## Scope

Build a standard-library Python supervisor around the existing Rust/offline
blueprint validation. This is tooling and documentation only. Implement in the
isolated bootstrap checkout supplied to the builder, not the original tree.

Read `docs/HANDOFF.md`, `docs/OVERNIGHT-CONTRACT.md`, the final candidate section
of `docs/IMPROVEMENT-LOOP.md`, `tools/blueprint/README.md`, `bp.py`, `perf.py`,
the actual crate manifests and source before coding. Verify installed Cursor
and OpenCode CLI flags against `--help`; never invent APIs.

## PHASE 0

Write `PHASE0.md` before implementation with a plan and substantive pushback.
Explicitly address authentication/sandbox limitations, fresh Cargo compilation
cost, snapshotting an extensively dirty checkout, independent-review failure,
and why naive bounded-channel replacement is unsafe. No flattering empty
disagreement section. Do not launch the overnight run yourself.

## Frozen contract

`docs/OVERNIGHT-CONTRACT.md` is read-only. Its gates and thresholds cannot change.
If a requirement cannot safely be implemented, record the blocker and stop
that path, rather than silently substituting a weaker mechanism.

## Deliverables (only these new paths may be edited)

- `tools/blueprint/overnight.py`: CLI and deterministic supervisor.
- `tools/blueprint/test_overnight.py`: standard-library unittest regression suite.
- `tools/blueprint/overnight-plan.json`: finite evidence-anchored candidate queue.
- `docs/OVERNIGHT.md`: analysis, loop design, exact commands, review workflow,
  safe stopping, overhead interpretation, and explicit limits.
- `PHASE0.md` and append-only raw-results sections in `docs/HANDOFF.md`.

Do not edit existing application/harness/config/dependency files. Do not create
an HTML dashboard, use remote publication or add packages.

## Required supervisor behavior

Expose `run`, `status`, `report`, and `stop` subcommands plus `--help`.
`run` supports explicit absolute `--source`, fresh absolute `--out`, default
`--hours 8`, `--closeout-minutes 45`, `--max-cycles 12` and bounded timeout
options. Include `--dry-run` that validates the plan, snapshots source and
writes truthful planning artifacts without running models or Cargo.
Do not use `shell=True`, shell-interpolated commands or destructive cleanup.

Discover source through Git tracked/untracked enumeration, preserving the
current bytes, not HEAD. Safely exclude generated/ignored artifacts and common
secret/user-data files. Refuse unsafe symlink/path input. Capture starting git
status, HEAD, per-file SHA-256, source manifest, immutable contract/plan hashes
and original release-executable hash. Create a local Git boundary with no
remote in experiment copies so parent Git discovery cannot reach the main
checkout. No commits or Git author changes. Preflight disk and tool availability.

Use an OS advisory repository lock and explicit run status. A second live
supervisor fails without touching the first run. Do not hold locks permanently
after crashes. PID ownership must not be inferred from a recycled PID alone.
Atomic writes for status/report; append-only JSONL events.

Run baseline gates before any builder. Every Cargo stage uses a run-owned target
directory, sanitized test environment, isolated HOME/FLINT_HOME outside `/tmp`,
and deadlines. Do not reuse/overwrite the source repository's release binary.
Baseline compilation may be costly; record it rather than hide it.

Each cycle begins from a preserved baseline or previous `proposed-validated`
experiment snapshot. Copy before modifying; preserve rejected candidates.
Write a bounded task prompt/spec with issue ID, evidence, changed-file allowlist,
the frozen safety contract, focused regression requirements, measured-scope
rules and PHASE 0. Builder: `cursor-agent --print --force --sandbox enabled`,
explicit experiment workspace. Use its existing auth as intended without
recording credential values. No `--approve-mcps`. Detect nonzero exit.

Reviewer: `opencode run --standalone -m opencode/deepseek-v4-flash-free`, private
server, strict read-only review prompt. Write its captured verdict separately
outside the candidate. Require a first-line `APPROVE` or machine-readable
explicit verdict, not substring matching in prose/tool chatter. If the configured
reviewer is unavailable, mark blocked and stop; never silently switch models.
Protect candidate source hashes across review, and do not let the reviewer
mutate implementation. Do not run additional agents or agents' own native
benchmark work concurrently with supervisor validators.

Diff actual snapshot bytes; enforce frozen-file/dependency/gate protection,
an eight-path application change limit and per-issue permitted paths.
Retain the per-cycle patch, cumulative patch, path/line-count metadata,
PHASE0 evidence and builder raw result. No automatic root patch application.
Run the frozen gates independently after building; validate stability afterward
and through review. Candidate failures are retained. Stop on serious contract,
drift or unavailable-review blockers; two consecutive failed/no-change attempts
stop useful work. No infinite retry. Record unresolved PHASE0 disagreement as
requiring architect/human disposition before promotion.

The loop is useful-work bounded, not a busy wait to claim eight hours.
Respect the closeout reserve; final gates run on the latest retained proposal.
Always produce an honest partial/morning report on deadline, stop or error.
Stop marks only this run, terminates/reaps owned subprocess groups, and preserves
evidence. Heartbeats let users distinguish a long compilation from a dead loop.
A default of twelve cycles is an upper bound, not a promise of twelve fixes.

Default native UI/performance checks are **skipped** because the original user's
app/child agents are active. Do not steal focus, change the open app or claim
native speedups. Include exact optional sequential offline blueprint commands
for an independently approved clean-host follow-up.

## Candidate queue and analysis

Use stable IDs, priorities, source anchors, permitted paths, hypothesis,
representative fixture, invariants and follow-up questions. Include:

1. Long-history tool-event lookup: realistic mixed parallel tool/subagent
   traffic, reference oracle, update latency and index memory/maintenance costs.
2. Persistence-pressure instrumentation/regressions: accepted-event ordering,
   bounded serialization batches as an experiment, failure retention, barriers,
   replay and shutdown. Do not implement delivery/queue-policy changes.
3. Subagent forwarding/cancellation observability: approval routing, retained
   work duplication, child completion, cancellation and resumability.
4. Harness orchestration debt: snapshot-isolated offline test entry points,
   stale/missing evidence, non-overlap and failed/missing-frame semantics.
5. Large task projection: realistic count/order/invalidation measurement,
   not the disproven first-eight iterator lead.
6. Documentation/API drift: verify persistence layout and orchestration claims
   against current source; patch only experiment docs with evidence.

Prefer fewer demonstrated fixes to speculative refactors. Follow-up candidates
must remain inside these approved areas and never invent a "tech-debt score".

## Oversight report

Markdown plus machine-readable JSON, generated from real run evidence:
architecture areas, current changes versus starting dirty tree, candidate ledger,
test/review commands and exits, skips/failures, source/binary provenance,
per-phase timings, wall/active overhead, attempted/proposed/rejected/no-change
counts, patch size, evidence bytes, remaining limits and next-check questions.
Historical results must be labelled historical, not copied into current tiles.
No HTML/publication required; augment the current blueprint, do not replace it.

## Acceptance tests (run before handing off)

```sh
python3 -m unittest discover -s tools/blueprint -p test_overnight.py -v
python3 tools/blueprint/overnight.py --help
python3 tools/blueprint/overnight.py run --help
```

Regression coverage must include: dirty/untracked snapshot fidelity, ignored
artifact exclusion, secrets/symlink/traversal refusal, immutable gates, source
drift, frozen/forbidden edit detection, exact reviewer verdict and unavailable
review behavior, failures/timeout, closeout budget, second-instance exclusion,
sanitized Cargo environment, no headless focus override, run-owned output,
owned-process termination and stop marker, append-only ledger/atomic state,
honest skipped/failed report, no-change/circuit-breaker behavior.

Use mocked or tiny fixture subprocesses for tests; no real models, credentials,
provider tests, Cargo compilation or native UI in the Python regression suite.
Append raw command exits/test counts/durations only to HANDOFF. The architect
will independently re-run tests and review the implementation before promotion.
