# Task outcomes and review

Flint now separates three facts: execution stopped, evidence was observed,
and the user accepted the result. A normal engine stop does not mean that a
deliverable is complete.

## Native workflow

- The host keeps the original request, current goal, requirements,
  restrictions and scope revision. Attachment content and image names do not
  become new user instructions.
- Use explicit `Required file:`, `Required check:` and `Acceptance:` lines
  when a requirement can be stated precisely. Unlabelled natural-language
  requirements remain in the original request and require review. Explicit
  requirements in later steering join the same bounded contract with stable
  identifiers; steering cannot silently remove read-only mode.
- Open **Task details** on the finished turn to inspect requirements,
  observed files, passing or failed checks, stale evidence, blockers,
  child results and the stop reason.
- **Edit scope** creates a guarded revision without replacing the original
  request. **Continue task** resumes that scope. Retry preserves the
  continuation operation, including a revised read-only scope.
- **Accept result** requires confirmation. It records the user's judgment,
  including any remaining gaps, not independent certification. Feedback
  buttons do not accept a task.
- Scope changes, observed file drift and undo invalidate prior acceptance.
  A review made against an older scope or evidence revision is refused.

The sheet has a bounded scroll area and a persistent action footer. Historical
turns and child details are read-only. Saving, restoring, running, shutdown and
storage failures explain why controls are unavailable. Closing the sheet does
not release an outstanding history writer. A failed scope save keeps the
editor draft; fields are disabled while a save awaits acknowledgment.
If continuation setup fails, the sheet closes to reveal the recovery action.

## Outcomes and queued work

| Outcome | Meaning | Automatic queue advancement |
| --- | --- | --- |
| Answered | A conversational answer, not an accepted deliverable | Allowed after a normal stop |
| Needs review | Observed evidence still needs user judgment | Held |
| Partial | Missing, failed, stale or unknown evidence, or an incomplete execution | Held |
| Blocked | Unresolved reported blocker | Held |
| Unknown | No trustworthy native assessment | Held |
| Accepted | Explicit acceptance of the current scope and evidence | Allowed after review |

A held queue keeps its prompts. Reviewing an unresolved result does not clear
the hold. **Resume anyway** confirms one dispatch without accepting the earlier
task. Reload never automatically resumes persisted queued work.

The default remains manual review. The opt-in Settings preference
**Continue queued work after passing checks** (`queue_after_verification = true`)
also allows a native **Needs review** result to advance when its assessment and
verification match the current scope and work revision, with no unresolved
issues, blockers, open plan steps or evidence overflow. It never records user
acceptance. Manual pause, reload pause, storage errors, external/legacy results,
failed execution and incomplete evidence still prevent automatic dispatch.
Approval mode and sandbox policy are independent and unchanged.

## Evidence and restrictions

Commands are recorded by exact identity, scope revision, shared work revision
and observation order. A later observed mutation, including concurrent child
work, makes earlier checks stale. A command's own completion does not by itself
invalidate that command's result; intervening work still does. A successful
unrelated command cannot erase
a failed required check. Child transport success is not task acceptance.
Successful child reports that need semantic review do not become failed
dependencies; the parent's evidence still needs current verification.
Failed exploratory commands remain inspectable in the transcript without
poisoning an otherwise verified task. Failed verification and exact required
checks remain unresolved until the same check passes.

Verification and command effects share one classifier. Formatting checks such
as `cargo fmt --check`, type checks such as `npx tsc --noEmit`, and simple
foreground inspection/check chains do not invent a later source edit.
Mutating formatters, unknown scripts, expansions and output redirection remain
conservative. Pipelines and shell conditionals do not automatically certify a
hidden check's exit status. This classification labels evidence; it does not
authorize a command or relax read-only policy.
Inspection history cannot consume all check slots and hide a later validation.
Protected check overflow still remains explicit and incomplete.

An agent may explicitly resolve its own previously reported blocker using
`report_task.resolved_blockers`, with the exact old text and a current passing
check. Merely omitting blockers does not clear them. User-review blockers
cannot be cleared by the agent, and resolving a report does not remove
requirements or accept a task.

Native read-only mode refuses edit tools, MCP tools and shell commands outside
a conservative inspection allowlist. Queued restrictions apply immediately,
including while a tool waits for approval or another operation is deferred.
Shell arguments are decoded before option checks; quotes do not hide write
or executable options.
Read-only mode is a tool policy, not an OS security boundary.

### Analysis without an editing workflow

Analysis-leading requests return findings in chat without writing a report or
running executable tests. The current protected scope overrides earlier edits,
checks and report files in the same conversation. Unavailable edit/MCP tools
are omitted from the current model request, but the execution boundary still
rejects stale tool calls.

`report_task.proposed_actions` records optional follow-up work separately from
blockers. It does not authorize changes, remove requirements, clear blockers or
record acceptance. Task details labels it **Suggested next steps (not applied)**.
A clean, normally completed read-only review displays **Analysis ready** while
its actual assessment remains **Needs review**. It is not automatically verified
or accepted.

A narrowly matched, host-observed denial of an unrequested local report edit
or executable check cannot by itself become a blocker to an analysis-only
contract. Such a report is rejected with guidance to return findings and offer
the optional action. A matching blocked final response gets a bounded correction;
repeating it without progress stops incomplete. Other blockers, explicit
requirements, user-review blockers and existing blocker-resolution rules remain
protected. Denied commands stay visible but are not credited as executed checks
or unknown shell effects. Previously saved outcomes are not rewritten.

File evidence uses workspace-resolved regular files and bounded 8 MiB FNV-1a
fingerprints. Special files are refused without waiting for a FIFO writer.
File scans and saved native projections run off the polling/UI thread.
Identical same-turn projections and unchanged review holds avoid repeat updates;
failed queue writes still retry.

Observation caps are explicit: 32 requirements, 64 checks, 64 files and 32
blockers. Overflow remains unknown rather than silently dropping a requirement
or failed check.

## Persistence and external agents

Native history version 2 stores messages and task state in one atomic snapshot.
Explicit native reviews and scope edits require a completed persistence
checkpoint before acknowledgment. Regenerated system/task context is not
duplicated in the saved message list.

Version 1 histories and older UI logs remain readable, without retroactive
acceptance. A newer native task can reconcile a missing UI event after restart;
missing answer text is never invented. If native history is missing, unreadable or its
turn counter is behind the transcript, continuation is refused to avoid reusing
turn identifiers or losing context. Keep the saved session for reading and
start a new session, or repair its history and reopen before continuing.
Damaged projection reads preserve the file and disable task controls. The
native engine's existing corrupt-history quarantine remains in place.

An interrupted JSONL tail stays on disk; appending inserts a record boundary.
Replay parses records individually, so even an incomplete Unicode character
cannot hide valid earlier or later records. A later confirmed history save
clears an earlier save-failure state, but not a native-history conflict.

ACP agents own their execution loop. Their normal stop is **Unknown**, not
native verification. Local acceptance records user review only; native scope,
read-only policy and evidence checks do not govern an external process.
Terminal cancellation closes public prompt intake before notifying the UI.
If an adapter removed a saved model/provider option, an explicit successful
identity selection can durably replace the disappeared slot, including if
the app quits before another prompt. Invalid values in slots
that still exist continue to block prompts.

## Verification and limits

Deterministic native-loop, persistence, ACP and headless GPUI regressions cover
the behaviors above. `tools/blueprint/tasks.py` captures ten synthetic task
states at two window sizes using an isolated release binary. It checks
outcomes, controls, settled inactive paint bounds and nonblank screenshots.
Existing task artifacts are refused rather than reused as current-launch
evidence; completed root paint is required as well as control geometry.
These rendering fixtures do not execute inference or prove task correctness.

Routine guard corrections are working guidance, not approval requests.
Recovered guidance and JEV evidence remain available under the work disclosure.
A normal stop is labeled **Turn ended** unless the result is answered or
explicitly accepted; it does not display a green completion claim above a
blocked or partial result.

The closeout evidence and exact executed results are retained under
`target/task-outcomes-20261005-g7gsefcf/`. Earlier failed runs are retained too.
The follow-up gap audit and its separate evidence are retained under
`target/task-gaps-20261005-71b_9bwn/`. Consult the relevant validation report
before treating historical gates as current.

Not certified: arbitrary semantic completion, every external filesystem
change, filesystem race freedom, side-effect-free repository checks, physical
keyboard behavior or VoiceOver compliance. Native captures use the existing
fixed application dark theme. Historical captures predate the Hooter rename.
