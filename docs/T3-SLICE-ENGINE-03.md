# T3 engine slice 03: bounded records and replaceable progress

Status: preparation only, no implementation authorization.
B1 independent acceptance and the B2 executor/supervisor contract must precede
the exact PHASE 0 and frozen execution gates. This document enables no capability.

## Current source boundary

Native `lib.rs:179–180` exposes unbounded legacy ops/events. `session.rs:88–165`
handles Interrupt/Approval on a separate task; simply bounding its message queue
would let an awaited Send block those controls. `subagents.rs:201,265–282` uses
an unbounded child channel and forwards with `try_send`, retaining tool work.

ACP `lib.rs`, `shared.rs` and `terminals.rs` also emit through existing unbounded
channels/`try_send`. Adapter integration belongs to its separately gated slice;
native-only bounds cannot be advertised as ACP/child bounds.

App `store.rs:125–265` has one ordered EventWriter: an unbounded command channel,
retained failed batches, append coalescing only before barriers, explicit Flush
retry and Stop/join. Replacing its channel or dropping a failed batch would
lose accepted data or deadlock the GPUI thread. Preserve it until an exact
nonblocking, ordered projection/migration design is separately authorized.

New contracts already distinguish immutable root-sequenced EngineRecord from
replaceable ProgressUpdate and lazy AgentRecordPayload/AssetRef. B1 declares
those types but implements no executor/delivery events. New enum boxing is
serde-transparent under the frozen B1 repair addenda.

## Proposed semantics requiring exact PHASE 0

Use one canonical supervisor/event producer, not a lossy forwarding layer after
an unbounded legacy bus. Durable critical records and full authoritative
content must be recoverable before notifying a bounded delivery cursor.
Progress is only a replaceable preview; it cannot become final transcript,
task acceptance, steering acknowledgement or an effect/approval receipt.

| Input | Semantics |
|---|---|
| Text/Reasoning/ToolOutput deltas | Concatenate authoritative content, or persist bounded chunks; coalesce only revisioned preview |
| TerminalOutput append | Preserve append ordering and authoritative output |
| TerminalOutput replace | Explicit same-terminal replacement; not append or last-delta loss |
| Usage | Latest cumulative snapshot for the same run/generation, not repeated addition |
| Turn/step/task/tool/approval/child/terminal boundaries, errors, options, files reverted | Ordered critical record; no best-effort `try_send` loss |

Each progress key binds conversation, run, row/call/terminal, kind and step/
generation. Invalidate obsolete generations before publishing a new step or
sealed row. Reused call/terminal names cannot carry stale progress into a
different run/child. Final promotion resolves complete canonical content,
preserving source offsets and UTF-8 boundaries for a retained suffix.

Frozen shared defaults are ceilings to enforce, not evidence of enforcement:
256 record refs, 128 progress keys/1 MiB per root, 8 MiB outbox metadata,
100-row/256 KiB query pages, four active/thirty-two retained children.
Include root plus child entries and real serialized bytes. Oversize critical
bodies become validated immutable lazy objects, not unbounded channel payloads.
Define fixed per-key suffix, IDs/nesting limits and behavior for full budgets
before implementation; never silently discard a critical event.

The PHASE 0 must name the exact source of final text, failure-retained spool/
cursor protocol, delivery acknowledgement, disk-full behavior and event-order
recovery. A second authoritative history saver or shared mutable record buffer
is not acceptable. UI replay/live folding consumes one authoritative route
only; no legacy+EngineEvent double fold or duplicated user prompt.

## Control and persistence constraints

Stop sets the owned cancellation token before awaiting storage/delivery. Parked
approval replies remain addressable and independent of progress/disk pressure.
Never hold task/WorkClock/approval mutexes across a potentially blocked write or
bounded send. Child cancellation/finalization remains separately attributed.

Failed durability produces a visible held/uncertain engine state, not terminal
success or task acceptance. The volatile storage-failure notification route
itself needs a finite, nonrecursive policy; a full queue is not permission to
weaken approvals, bypass a side-effect barrier or redispatch uncertain work.

## Intended evidence, not passing results

- Slow/not-draining receiver with large Unicode/fence/terminal streams retains
  exact final source and every critical boundary in root sequence order.
- Measured retained counts/serialized bytes stay under exact root-wide caps;
  adversarial distinct IDs cannot grow an unbounded key/cache map.
- Step/run/child/terminal replacement races reject stale progress; terminal/
  row seal and task assessment arrive before any eligibility transition.
- Blocked storage and full delivery still permit stop/approval intake without
  a held-mutex deadlock; failed spool/commit emits no successful acknowledgement.
- Crash/lost-delivery/reopen reconciles the durable cursor without duplicate
  task/tool/terminal events or implicit rerun.
- Legacy EventWriter append/retry/flush/stop ordering and existing native/
  child/ACP regressions remain intact; no whole-system bounds claim from a
  partial adapter migration.
- Focused offline tests, strict affected-crate Clippy and stable-source
  integration evidence pass under the existing shared slot.

Ownership, real APIs, exact limits and any necessary B2/B3 subdivision must be
ruled on before code. No new backend worker or execution follows from this
preparation while B1/F2a/R0E are at their current boundaries.
