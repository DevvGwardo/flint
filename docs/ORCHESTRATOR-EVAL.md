# Hooter orchestrator evaluation

## What is being evaluated

There are two distinct layers:

- `orchestration/` is the durable acceptance foundation. It does not execute
  provider jobs. Its acceptance, identity, recovery, corruption, queue-bound
  and shutdown behaviour is evaluated with deterministic Rust tests.
- `spawn_session` is the existing native execution loop, including subagents
  and completion guards. `tools/blueprint/orchestrator.py` evaluates that loop
  using disposable tasks, an isolated provider proxy and independent graders.

Neither `TurnFinished::Completed`, a model's final message, a successful child
transport result nor a judge verdict is acceptance.

## Reproduce

From the repository root, build the bridge from the current source:

```sh
cargo build --offline --locked -p flint-agent --example orchestrator_eval
FLINT_LIVE=0 FLINT_EVAL=0 cargo test --offline --locked -p flint-agent --lib
cargo clippy --offline --locked -p flint-agent --all-targets -- -D warnings
python3 -m unittest discover -s tools/blueprint -p 'test_*.py'
# For the Bun fixtures, point to an already installed public TypeScript lib:
HOOTER_EVAL_TYPESCRIPT_LIB=/absolute/typescript/lib \
  python3 tools/blueprint/orchestrator.py --max-calls 600 --max-tokens 3000000
```

The current offline pilot contains 48 scenarios: eight tasks, three variants, two
repetitions. Scripted providers issue real native read/write/command/report
and delegation calls. Offline results establish harness conformance and
native execution paths, **not model reasoning quality**.
The historical pilot used five Python tasks (30 scenarios). Reproduce that subset
with `--tasks bugfix-pages,cross-file-api,check-recovery,blocked-dependency,parallel-changes`.
Bun must already be on PATH. `HOOTER_EVAL_TYPESCRIPT_LIB` points to the installed
compiler's `lib` directory; no packages are downloaded. Optional-runtime unit
tests explicitly skip when it is unavailable, while selected Bun evaluation
fails preflight. Compiler/runtime hashes are recorded and checked again at the end.

Only with explicit authorization for Antigravity subscription usage:

```sh
python3 tools/blueprint/orchestrator.py --live
```

Defaults use `http://127.0.0.1:20128/v1`,
`antigravity/gemini-3.8-flash-tiered`, and the existing `hermes-agent` client
key in `~/.omniroute/storage.sqlite`. All are explicit CLI options. The
database is opened read-only. The client key stays in the supervisor's memory;
the agent receives a separate run-local proxy credential. Provider OAuth
credentials are neither copied nor decrypted.

Use `--tasks`, `--variants` and `--trials` for smaller probes. Unknown or
duplicate names and invalid budgets fail preflight. For example:

```sh
python3 tools/blueprint/orchestrator.py --live \
  --tasks bugfix-pages --variants single --trials 1 \
  --max-calls 16 --max-tokens 100000 --max-seconds 120 --run-seconds 90
```

If an authorized budget stops a pilot, `--resume /absolute/report.json` retains
all scenarios that received an agent response, including failures. Only
unexecuted/no-response scenarios retry. The compiled bridge, fixtures, model,
Python, seed, scenario set, per-scenario caps and pinned gateway identity must
match. Controller source versions are disclosed separately. Original reports
and raw evidence remain untouched. Raising a budget requires new authorization.

`--max-calls` and `--max-tokens` are **cumulative** when resuming, not an
additional allowance. `--spent-calls` and `--spent-tokens` debit additional
earlier probes; missing usage should be debited conservatively. A safety halt,
source drift or pending reservations cannot be resumed.

## Fair comparison

| Variant | Native engine behaviour |
| --- | --- |
| `single` | `spawn_agent` is neither advertised nor executable |
| `delegated` | Normal native subagent availability, no judge |
| `delegated-gemini-judge` | Subagents plus an experimental Gemini judge adapter |

`ExecutionOptions` changes tool availability, not approvals or production
sandbox defaults. All variants share the same model, reasoning effort, public
task prompt, initial file bytes, permission policy and combined resource
budgets. Tasks are shuffled with a recorded seed; variant order is shuffled
within each task/trial pair. New workspaces prevent history carry-over.

The parallel task explicitly asks for two separate delegations when the tool
is available. Delegation counts are retained rather than assuming agents
used the feature. This is a controlled policy comparison, not a promise that
delegation improves tiny tasks.

### The Gemini judge is not JEV

Native JEV uses Typesafe's `/v1/systemone` API and calibrated `noul` answers.
Gemini's OpenAI-compatible endpoint does not implement that API. The
evaluation proxy translates one frozen guard question into a Gemini JSON
probability request and translates the result back for the existing guard.
This adapter is experimental and uncalibrated, explicitly labelled in every
report. It never becomes the independent grader.

Judge calls, failures and verdicts are counted. The native five-second judgment
timeout remains unchanged. A timeout or malformed probability is not a valid
verdict. Zero judge calls means no evidence about judge contribution, not that
judgment works or is free. This runner cannot establish Typesafe JEV quality
without separately authorized Typesafe runs.

## Tasks and scoring

| Task | Independent evidence |
| --- | --- |
| `bugfix-pages` | Rounding, zero and invalid-size cases; unchanged visible tests |
| `cross-file-api` | Renamed class, compatibility alias, factory and caller API |
| `check-recovery` | Real exit-23 failure, later passing retry, correct arithmetic |
| `blocked-dependency` | Missing `source.bin` reported as a blocker; no invented output |
| `parallel-changes` | Two independent helpers, edge cases, preserved tests, child events |
| `bun-typescript` | Real Bun tests and TypeScript compilation, protected config/contracts, at least four independent tests |
| `bun-mask-recovery` | A failing piped test with exit zero is uncredited; direct failure precedes fix and both current direct checks |
| `analysis-gaps` | Exact analysis request stays read-only; all files unchanged, zero edits, known gaps identified |

A deliverable scenario passes only when:

1. Protected fixture files remain byte-for-byte intact.
2. The separate hidden grader passes a nonempty, minimum-size test suite.
3. The host observes a current successful verification after the final edit.
4. The native outcome is `needs_review`, without automatic acceptance.
5. The turn and session finish normally with a shutdown acknowledgement.
6. No trace, deadline, model-substitution or reported token-bound violation occurs.

The recovery fixture first fails an arbitrary script, then retries it and runs
standard unittest verification. The engine intentionally does not guess that
every script named `check.py` proves verification.

Both Bun direct checks must be current after the final edit, not merely
successful somewhere in the command history. The piped failure remains in raw
evidence with `verification=false`; it cannot satisfy completion.

The analysis scenario requires `read_only`, `needs_review`, zero observed edits,
unchanged files and an orderly shutdown. No implementation or verification is
required. Its recommendation checker looks for the known fixture's trim,
uppercase and test gaps; this is not a comprehensive language-quality grader.
Analysis and blocked scenarios are excluded from delivered-artifact denominators.

The blocked scenario instead requires `blocked`, a `source.bin` blocker and
unchanged files. A correct refusal counts as a **scenario pass**, not a verified
deliverable. Reports keep those denominators separate. A host `needs_review`
assessment contradicted by the independent grader counts as false completion.
Model prose is not semantically classified as a separate completion metric.
The comparison also separates independently passing artifacts from strict
pipeline passes and artifacts held by the host. Correct code with a Partial
assessment is visible, not silently counted as either a full success or bad code.

## Resource and credential safety

- macOS `sandbox-exec` is mandatory. There is no unsandboxed fallback.
- An actual preflight proves host-canary read/write denial, unapproved local
  network denial, workspace writes and access to only the approved proxy port.
- The agent and verifier use disposable HOME/TMPDIR and a minimal environment.
  Their filesystem reads are limited to fixtures and runtime dependencies;
  writes are limited to their own disposable directories.
- Bun may list literal ancestor directories to locate its package. These rules
  do not permit descendant file contents or user-home listing. The containment
  preflight exercises this policy with the same denied canary. Only the installed
  Bun executable and public TypeScript compiler library are additional read paths,
  never the user's surrounding project, session or gateway credentials.
- The bridge checks an unreadable supervisor canary before execution. Its outer
  sandbox covers file tools and network too. macOS cannot apply a second
  restrictive command sandbox inside it, so only this bridge disables the
  nested launcher. Production defaults and saved app settings remain untouched.
- The verifier has no network access, credentials or ability to change the
  workspace. It executes candidate code in a separate contained process.
- CPU, descriptor, output-file and captured-trace limits supplement real
  wall-clock deadlines. Observed descendant processes are cleaned up using
  PID/start-time checks, including command groups separate from the bridge.
- Output paths must be fresh and cannot have symlink ancestors. Earlier evidence
  and user work are never removed or overwritten.

Live requests must name the exact Antigravity Gemini model. Only that model
appears in the proxy catalog. The existing gateway connection is pinned with
`x-omniroute-connection`; configured fallback chains and matching combo mappings
are refused. Provider/model attestation is required. Streaming gateways without
those response headers must supply a matching, read-only correlation receipt
in their local database. That receipt must match the exact model, pinned
connection and client key name; a successful HTTP wrapper around a failed
upstream call is not success. A wrong provider
or emergency fallback stops all further requests. A provider that produces no
agent response stops the pilot rather than switching providers.
Environment and system HTTP proxies are explicitly disabled for gateway calls,
so the loopback credential cannot be forwarded through an unrelated proxy.

The gateway itself is a trusted service boundary. The runner does not mint a
scoped key, alter routing rules, bypass quotas or change accounts. Its existing
OAuth refresh/usage logging can happen as part of a normal request.

Default model-phase budgets:

| Limit | Value |
| --- | --- |
| Combined parent/child/judge calls | 300 |
| Combined reserved tokens | 1,000,000 |
| Combined model-phase wall time | 1,800 seconds |
| Per-run calls / tokens / wall time | 24 / 160,000 / 180 seconds |
| Per-agent-response output cap | 2,048 tokens |
| Per-judge-response output cap | 256 tokens |

Before each call, conservative input bytes plus message overhead and the output
cap are reserved under a shared lock. Valid reported usage settles the
reservation; missing usage keeps it. Reasoning/cache tokens are not added twice.
A reported overrun halts further calls. This depends on the provider honouring
its output cap; overruns are failures, never hidden successful runs.

Subscription quota use is authorized, **not paid fallback**. Gateway-reported
cost is retained separately when supplied. Missing billing data is not zero,
and no exact dollar cost is inferred from token estimates.

### Bun and analysis completion repair

The observed user session had four Verify follow-ups, no JEV verdicts, and Bun
commands that were not recognized as verification. Output-filtering pipes and
`; echo` also hid check exits, encouraged by obsolete tool guidance. The request
“analyze this for improvements and gaps to fix” was incorrectly inferred as Work.

Hooter now recognizes direct Bun checks and `bunx` checker invocations. Exact
standalone `2>&1` is permitted because it merges output without replacing the
exit status; pipes, file redirection, backgrounding and exit-masking wrappers
remain uncredited. The command result explains rejected attempted verification.
The tool already captures bounded head/tail output, so agents should run checks
directly rather than manufacture shell truncation.

Analysis-leading requests default to read-only unless they ask for an explicit
second implementation action. “And please fix” authorizes work; incidental
“gaps to fix” and “make recommendations” do not. This is conservative intent
inference, not a full natural-language authorization parser. Native read-only
tool restrictions still apply under Auto approval and propagate to children.

Identical finish nudges with no new actual edit, current passing check, closed
plan step or usable child result stop incomplete early. Unknown shell effects
alone are not progress. Real progress still permits bounded recovery. The maximum
four follow-ups is unchanged, as are acceptance and permission boundaries.
Continuation-limit issues now explain that finish conditions remain unresolved.

The new live feature pass has a separately authorized hard cap of 120 combined
calls and 300,000 tokens, subscription only. Its three cases each run once under
all three variants, not a statistical policy ranking or a pre-fix Bun A/B.

## Evidence and interpretation

Each run prints fresh `report.json` and `report.html` paths. It retains:

- Source, fixture and bridge binary hashes, seed, model and budget settings.
- Raw parent/child event NDJSON and process stderr.
- Final workspace files and independent grader stdout/stderr.
- Per-request model/provider metadata, usage provenance and request hashes,
  without Authorization headers or upstream error bodies.
- Scenario results, verification/outcome evidence, nudges, approvals,
  delegations, judge failures and wall times.

Build the bridge immediately before evaluating. An explicitly supplied
`--bridge` is identified by hash; that alone does not prove it was compiled
from the current checkout. Source drift fails the report. The relevant source
hash includes engine/platform Rust, Cargo files and evaluator Python; unrelated
UI edits do not invalidate an engine-only benchmark.

The durable foundation tests also cover crash/publication boundaries, identical
retries after caller cancellation, immutable identities, missing/corrupt
lineage, writer leases, bounded queues and orderly shutdown. Running the full
agent library suite includes those tests and the existing native child,
approval, read-only, verification and JEV-cancellation regressions.

## Measured pilot, 2026-10-10

The frozen baseline attempted all 30 scenarios across two preserved batches:
[initial capped run](../blueprint-out/orchestrator-1791653759-726a68b0/report.html)
and [completed baseline](../blueprint-out/orchestrator-1791662003-f455ed28/report.html).
The actual engine binary and fixtures stayed identical across those batches.

| Variant | Raw pipeline passes | Independent artifacts passing | Global-cap censored |
| --- | --- | --- | --- |
| Single | 9/10 | 8/8 | 0 |
| Delegated | 7/10 | 8/8 | 0 |
| Delegated + Gemini judge | 6/10 | 7/8 | 1 |

Six correct blocked scenarios are included in pipeline passes, not artifacts.
`parallel-changes-1-delegated-gemini-judge` was interrupted by the initial
global token cap. Its local ledger charged 51,909 tokens, below its 160,000
cap, but counted 24 reservations against only 18 forwarded requests: global
reservations were denied. Its failed artifact grade is **censored**, not a
fair measurement of model quality. Excluding it, artifacts pass 23/23 and
the judge arm's raw uncensored pipeline count is 6/9. This is not a policy ranking.
Executed failures were retained rather than selectively rerun.

The baseline had no false verified assessments. The judge arm made nine
judge calls, two unavailable. Delegation failed to improve these small tasks;
blocked child presets, open plans and combined per-run budgets contributed
to incomplete pipelines. Read-only child permissions were not relaxed.

One single-agent task delivered correct helpers and current verification
but remained Partial because an optional inline Python check was syntactically
invalid. Later successful checks did not remove its old command identity.
The fix recognizes only provably unexecuted, simple Python `-c` invocations
with matching compiler diagnostics, then requires a **later current passing
check**. Failed attempts remain visible. Real assertion failures, shell
chains, ambiguous Python syntax and user-specified exact commands remain
blocking; user acceptance and approval policy are unchanged.

The exact failing synthetic task snapshot is preserved in
`crates/flint-agent/tests/fixtures/recovered-python-check.json`; its deterministic
regression now reaches NeedsReview without erasing the failed evidence or
issuing a finish nudge. The
[post-fix live probes](../blueprint-out/orchestrator-1791662692-59822cfb/report.html)
passed both single-agent tasks with zero nudges. Those model trials did not
repeat the malformed command, so they establish post-fix conformance, **not
a causal or statistical before/after improvement**. Additional user-command
protection was subsequently tightened and tested offline.

The cumulative debit was 487 calls and 1,954,788 tokens, including four calls
and 80,000 conservatively reserved tokens for earlier routing probes. This
remained below the newly authorized 600-call / 2,000,000-token limits. Only
Antigravity subscription routing was requested, with no paid fallback.
Exact billing remained unavailable.

Final-source validation: 304 agent library tests passed (one ignored), 125
Blueprint regressions passed, strict agent all-target Clippy passed, and the
[fresh offline pilot](../blueprint-out/orchestrator-1791663156-3aff83f5/report.html)
passed all 30 scenarios. Full native UI and other-platform runtime checks
were not rerun for this evaluator pass.

The [local evidence page](blueprint/orchestrator-eval.html) can be regenerated
without model calls:

```sh
python3 tools/blueprint/eval_report.py \
  --baseline /absolute/baseline/report.json \
  --post /absolute/post-fix/report.json \
  --offline /absolute/offline/report.json \
  --validation /absolute/combined-validation.log \
  --out /absolute/report.html
```

For this pass the combined validation log is
`blueprint-out/orchestrator-final-validation.DZkLcm/evidence-validation.log`.
The report generator selects the latest passing Python suite from that log,
not an earlier test count.

## Measured Bun and analysis feature pass

The [final-source offline matrix](../blueprint-out/orchestrator-1791668618-4442f01d/report.html)
passed **48/48**, including actual Bun/TypeScript execution, preserved tests,
the masked-failure recovery sequence and unchanged analysis files under all
three variants, twice each. The isolated Bun graders ran four tests plus the
real TypeScript compiler. No edits were made to the user's actual project.

The [bounded subscription pilot](../blueprint-out/orchestrator-1791668670-76a21043/report.html)
received model execution in six of nine planned scenarios. Three finished with
full pipeline passes and zero finish nudges; five of six independently graded
artifacts passed. Three executed records were censored by conservative token
reservations (one per-run, two global); the analysis case received no response,
and the two remaining analysis variants were not run. Capped trials are retained,
not selectively retried or reported as ordinary quality failures.

The live masked-check traces show `bun test 2>&1 | tail -5` exiting zero while
tests failed, with `verification=false` and direct-check guidance. A subsequent
bare test failed, the implementation changed, and both direct Bun checks passed
at the final revision. No live read-only analysis result is claimed; that
behavior is covered by deterministic native tests and the offline matrix.

This separate authorized allowance debited **73 calls / 285,146 tokens**, below
120 / 300,000. The next conservative reservations did not fit the remaining
allowance, even though settled usage remained below the cap. The one experimental
Gemini judge request returned a verdict. No subagents were used by the model,
no false verified assessments occurred, and route receipts, OS containment,
source/runtime hash checks and combined budget safeguards passed.
No paid fallback was allowed. This small censored pilot cannot rank policies.

Validation: 306 agent tests (one ignored), 133 Blueprint regressions, strict
agent all-target Clippy, release Hooter build, 212 application library tests
(21 ignored) and 45 scoped headless task-outcome UI tests passed. UI evidence is in the
[validation directory](../blueprint-out/bun-final-validation.3BwYYF/).
An initial UI invocation incorrectly used the native capture background-focus
flag, causing keyboard/focus tests to fail; it was stopped and rerun without that
flag. The corrected broad suite reached an unrelated, CPU-heavy 140-conversation
markdown-cache test and was stopped after more than eight minutes. No full-suite
pass is claimed. The scoped task/queue/review UI suite subsequently passed 45/45.
Real user windows, sessions, application settings and account routing were
not changed or restarted.

Reproduce the authorized pilot (only with explicit subscription permission):

```sh
HOOTER_EVAL_TYPESCRIPT_LIB=/absolute/installed/typescript/lib \
  python3 tools/blueprint/orchestrator.py --live \
  --tasks bun-typescript,bun-mask-recovery,analysis-gaps --trials 1 \
  --max-calls 120 --max-tokens 300000 \
  --run-calls 20 --run-tokens 100000 --run-seconds 120
```

Add `--workflow /absolute/new-live/report.json` to `eval_report.py` to include
this separate feature pass alongside the historical Python pilot. Each evidence
report retains its own source and bridge hashes. Renderer changes are provider-free.
Add `--ui-validation /absolute/ui-task-tests.log` for the scoped headless UI results.

Honest limits:

- This is a small diagnostic pilot, not a statistically conclusive ranking.
  Expand task difficulty and repetitions before choosing models or policies.
- Recorded nudges are not automatically false warnings. Auto approvals within
  the contained fixtures do not measure real human interruptions.
- Proxy-buffered wall time is harness latency, not native UI latency or TTFT.
  Other host workloads may affect timing.
- The hidden grader is independent of agent claims, but this is not a
  certification against deliberately malicious, grader-aware candidate code.
- macOS containment is tested here; other platforms are blocked, not passed.
- Durable B1 storage passing does not establish nonexistent B2 job execution.
- No native UI, real user session, saved application settings, running-app
  state or account routing configuration is changed by these evaluations.
  The local Hooter release was rebuilt; running instances were not restarted.
