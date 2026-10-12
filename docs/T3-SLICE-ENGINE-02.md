# T3 engine slice 02: native scheduling and side-effect barriers

Status: prepared, not authorized. B1 independent acceptance is a prerequisite.
No worker may start B2 from this document alone.

## Required preimplementation checkpoint

After B1 acceptance, the backend worker must verify its implemented APIs against
`T3-CONTRACTS.md` and propose the exact native executor/supervisor integration,
numbered disagreements, file ownership, tests and any necessary bounded
subdivision. The architect freezes the execution slice before implementation.
Do not treat this preparation as permission to expose a scheduling capability
or modify frozen shared contracts.

## Intended scope

Integrate the existing native executor with one canonical supervisor, not a
second agent loop or independently authoritative saver. Keep legacy public
`Op`, `AgentEvent`, `AgentConfig` and `SessionHandle` shapes and ordinary legacy
behavior. New orchestration events and legacy projection must never both fold
the same session into the app.

- Durable acceptance stays separate from start. Commit the immutable run
  identity, turn, exact request and frozen effective context before provider
  dispatch. NextTurn must not become opportunistic steering.
- NativeSafeBoundary is acknowledged as started only at actual durable
  instruction injection. Finish it with the containing turn, not intake.
  StopThenSend explicitly waits for cancellation and holds uncertain work.
- Route native task edit, review and continue through committed command
  transitions. TaskRecord, WorkClock, filesystem drift, approvals and
  recheck-after-approval remain authoritative.
- Implement atomic queue cancel, fresh-ID replacement, reorder, pause and
  explicit resume against expected revisions. Accepted edits are never
  in-place payload mutations.
- A nonserialized one-use continuation permit binds command, conversation,
  task/work revisions and restart generation. It permits one next command,
  not acceptance, replay of uncertain effects or permanent review bypass.
- Side-effect Prepared/Dispatching/Completed barriers are immutable commits.
  No mutating or externally effectful invocation may pass a failed barrier.
  Unknown shell/MCP effects are conservative, not declared read-only.
- Controls and cancellation remain independent of command/delivery/storage
  backpressure. Stop acknowledgement is not proof of effect rollback.
- Preserve restart holds and never automatically redispatch a Started
  identity. Storage failure before invocation prevents it; failure after
  invocation holds the uncertain result without retry.

The PHASE 0 must explicitly cover existing child/tool paths that can invoke an
effect. Do not claim root-only barriers protect children. A temporary unsupported
capability in the new orchestrated path requires an explicit architect ruling
and visible rejection before effects; legacy child behavior must not regress.
New addressable child supervision, provider profiles/ACP, transfers, rich
rendering, secrets, watches and app outbox integration remain their later
independently gated slices.

## Intended acceptance evidence

1. Local fake-provider tests distinguish Accepted, Started and Finished and
   inspect the canonical request/run checkpoint before the first HTTP request.
2. Retry/lost-reply/restart tests invoke a command identity at most once
   automatically; Accepted recovery holds, Started recovery never redispatches.
3. NextTurn and actual safe-boundary injection remain distinct; authoritative
   request, context attachments, task assessment and WorkClock are preserved.
4. Review-needed, blocked, unknown and filesystem-drift outcomes hold the queue.
   Only Answered/Accepted advances normally. A scoped continuation expires on
   edit, work change, stop or restart and permits exactly one command.
5. Queue revision conflicts and failed cancel/replacement commits preserve old
   state. Same-ID retries cannot allocate another run or accepted replacement.
6. Fault injection surrounds intent/start/finish commits and effect invocation.
   Failed pre-effect persistence yields zero effects; post-effect failure is
   ambiguous, not retryable success. Covers actual native tool/approval paths.
7. Restriction tightening while approval is parked prevents a newly forbidden
   effect. Cancelling/replacing one pending command removes only its keyed
   pending restrictions.
8. Slow receiver and blocked storage do not prevent stop/approval intake or
   manufacture terminal/task acceptance. Root versus child effects and
   finalization remain correctly attributed.
9. Focused existing session/task/approval/steering/child regressions plus new
   orchestration integration tests pass offline; strict affected-crate Clippy
   and owned-file formatting pass without suppressions.

Use the existing pass-owned serialized runner and filtered environment. Only
local dummy credentials/providers and run-owned workspace effects are allowed.
Retain exact public signatures, source provenance, failed and passing command
logs and test counts. Backend reports facts; independent architect review and
validation precede any app integration or next slice.
