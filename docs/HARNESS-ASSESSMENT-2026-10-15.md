# Hooter harness assessment, 2026-10-15

## Recommendation

Worth improving as a personal, local, multi-model desktop workbench. There is
not evidence that its native coding loop outperforms Claude Code or Codex.
Keep the differentiated UI, provider choice, ACP interoperability, and explicit
task evidence. Do not try to match every competing runtime by adding more
judges, agents, retries, or background machinery.

Hooter is both a native harness and a frontend for other harnesses. Native
changes do not change Claude Code, Codex, or Droid behavior through ACP.
Rust/GPUI can improve local responsiveness; it does not establish better model
reasoning, task success, or lower inference cost.

## Ground-truth inventory

| Concern | Current implementation | Assessment |
| --- | --- | --- |
| Native execution | `crates/flint-agent/src/session.rs` | Real model/tool loop, not just a chat UI |
| Completion | `task.rs`, `harness/guard.rs` | Revision-aware evidence, acceptance separate from execution, bounded recovery |
| Delegation | `subagents.rs`, `worktree.rs` | Existing four-child/worktree support; not a reason to delegate every task |
| Context | `context.rs` | Token calibration, stubbing, short summaries; not lossless source retrieval |
| Tools | `tools/mod.rs`, `mcp.rs` | Native tools and MCP; no native cross-project activity/session-search tool in `TOOL_NAMES` |
| Persistence | `persist.rs`, `orchestration/` | Durable storage exists; the B1 orchestration entry point explicitly does not execute provider jobs |
| Local command sandbox | `tools/sandbox.rs` | macOS write confinement; reads and network remain open, not complete secret/network isolation |
| JEV | `harness/jev.rs`, `harness/mod.rs` | Optional advice with heuristic fallback; current task/model quality benefit unproven |
| Evaluation | `tools/blueprint/orchestrator.py`, `docs/ORCHESTRATOR-EVAL.md` | Existing independent graders and resource-bounded runner; reuse rather than create another platform |

The screenshot's wrong search scope and premature blocker were real correctness
problems. The preceding blocker-recovery change addresses that failure path,
but prompt guidance is not a project registry or an activity index.

## Comparison and patterns worth borrowing

These are architectural comparisons from local source and public documentation,
not a controlled product ranking.

| Reference | Useful pattern | Implication for Hooter |
| --- | --- | --- |
| [Claude Code agentic loop](https://code.claude.com/docs/en/how-claude-code-works) and [skills](https://code.claude.com/docs/en/skills) | Tool-driven execution and reusable task workflows | Prefer narrow reusable workflows and existing ACP agents over more native heuristics |
| [Codex approvals/security](https://developers.openai.com/codex/agent-approvals-security) | Separate sandbox, approvals, and network controls | Do not confuse a blocker judge or approval toggle with isolation |
| [OpenAI harness engineering](https://openai.com/index/harness-engineering/) | Make repository knowledge discoverable and mechanically verifiable | Improve available evidence and verification before adding autonomous complexity |
| [Aider repository map](https://aider.chat/docs/repomap.html) and [lint/test workflow](https://aider.chat/docs/usage/lint-test.html) | Ranked, bounded code context and concrete validation commands | Useful next step for large-repository context, not a reason for a broad rewrite |
| [Anthropic long-running harnesses](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents) | Explicit progress and testable completion across sessions | Keep durable task evidence and independent checks; summaries alone are insufficient |

## What local evidence actually says

The historical small-task pilot in `docs/ORCHESTRATOR-EVAL.md` reports raw
pipeline passes of 9/10 single-agent, 7/10 delegated, and 6/10 delegated plus
Gemini judge. One judge-arm scenario was globally capped. Independent artifacts
were 8/8, 8/8, and 7/8; the last denominator contains that censored case.
Those are not fair product rankings or proof that delegation is generally worse.
The judge was experimental Gemini, **not calibrated Typesafe JEV**.

These results do not justify unconditional delegation or JEV claims. Removed
the source comment implying another harness's 89% to 75% ablation established
JEV's benefit here. A real JEV decision needs paired, same-model/task/budget
trials with independent artifact grades, correct-block and false-block rates,
tokens, latency, and unavailable-judge counts. No live trial was run this pass.

## Implemented bounded optimizations

| Deterministic loopback reproduction | Before | After |
| --- | --- | --- |
| Six attempted judgments during a continuous HTTP 503 outage in one turn | 6 HTTP requests | 1 HTTP request, then deterministic fallback |
| Finish judgment after recovery budget is exhausted | 1 HTTP request | 0 HTTP requests |

The circuit breaker resets on a new turn. Normal six-call/five-second limits,
midturn loop judgments, protected blockers, scope, and verification remain.
Skipping an unavailable judge does not accept work. The request counts are
measured regression results, not a model-quality or end-to-end speed benchmark.
Previously `Harness::judge` used `ask_noul(...).await?`, so failure returned
without disabling later calls; it also checked only the JEV call budget, not
whether a finish follow-up was still possible.

The two new cost regressions failed before the fix (6 versus 1 requests,
and 1 versus 0); they pass afterward. Coverage also proves next-turn retry,
deterministic verification fallback, and preserved midturn judgments.

Validation: 450 agent unit tests passed, one manual performance test ignored;
strict agent all-target Clippy, changed-Rust formatting, app/test compilation,
and whitespace checks passed. Live/eval/benchmark opt-ins were disabled.
Their integration entry points returning successfully are not live-model results.

## Highest-value next work, bounded handoff

1. **P0: explicit read-only activity discovery.** Inspect `tools/mod.rs`,
   `prompt.rs`, `config.rs`, and app session metadata. Add opt-in known project
   roots and a bounded metadata-only session/activity query. No full-disk scan,
   credential directories, new write grants, or claim that clone/commit/file
   timestamps prove repository creation. Test empty scratch folders, inaccessible
   roots, result limits, source citations, and unchanged access controls.
2. **P1: paired evaluation before tuning policy.** Extend the existing Blueprint
   fixtures with false blockers, correct blockers, unavailable judges, and
   recovery budgets. Keep model, tasks, seed, and total budgets comparable.
   Do not use `tests/eval.rs` as a hard-timeout controller: its `recv_blocking`
   can outwait its surrounding deadline. The existing Blueprint bridge already
   uses `timeout_at` and shutdown acknowledgements. No live subscriptions,
   paid calls, or external benchmark submissions without explicit authorization.
3. **P1: source-preserving context retrieval.** Inspect `context.rs`,
   `tools/mod.rs`, and transcript storage. Prototype bounded source references
   and on-demand retrieval before a full repository-map service. Test retained
   task restrictions, stale references, token caps, and long-session recall.
4. **P1 before untrusted use: tighten isolation.** Design explicit read/network
   scopes with tests and migration guidance. Do not silently change current
   permissions or advertise the existing macOS write policy as a complete sandbox.

Non-goals this pass: changing permissions, completing B1 execution, modifying
external ACP engines, using real credentials, replacing the installed app, or
claiming superiority over other harnesses. Larger features above remain handoff
work, not implemented capabilities.

Handoff verification, from the repository root:

```sh
FLINT_LIVE=0 FLINT_EVAL=0 FLINT_BENCH=0 cargo test --offline --locked -p flint-agent
cargo clippy --offline --locked -p flint-agent --all-targets -- -D warnings
cargo check --offline --locked -p flint-app --tests
```

## Follow-up implementation

The subsequent implementation adds opt-in activity discovery through existing
MCP support (`tools/activity_mcp.py`), bounded historical read-source pointers
in compaction summaries, and legacy evaluation event deadlines/shutdown barriers
with separate artifact, host-assessment and JEV-verdict reporting.

See [harness-follow-through.md](harness-follow-through.md) for configuration,
boundaries, verification and the isolation design. Full OS read/network policy
enforcement, durable source retrieval, and authorized live paired quality
measurement remain open. No roots, permissions or installed app were changed.
