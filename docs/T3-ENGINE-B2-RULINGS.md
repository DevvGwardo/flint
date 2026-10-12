# B2 PHASE 0 rulings and exact-schema checkpoint

Status: next bounded scope selected as B2a, the private canonical root
execution seam. The same backend owner may prepare its exact private schema/
interface contract only. Implementation and validation await the primary's
separate freeze of that contract. No B2 public scheduling or provider/tool
execution is authorized now.

Proposal: run-root `specs/backend-b2-phase0.md`. B1 independent acceptance and
all historical/shared contract files remain unchanged.

## Disagreements D1–D12

1. **Accept subdivision.** B2a builds/exercises the actual existing native
   executor/checkpoint/intent seam privately. Public acceptance opens/spawn stay
   nonexecuting. B2b queue/task authority and B2c delivery/activation are later
   slices. A fixture-only foundation is not full implementation or app parity.
2. **Accept one canonical writer.** Use a private validated construction bundle,
   shared preparation/turn_loop/assessment helpers, and typed asynchronous
   checkpoints. Never invoke a second Saver/load/Subagents constructor or treat
   absence of Saver as successful persistence. Legacy behavior stays unchanged.
3. **Accept stricter request policy, with transport correction.** Native
   orchestration performs one dispatcher attempt for each committed exact
   request-body intent. No effort fallback, dropped-stream resend, provider
   retry or automatic redirect. Subsequent legitimate successful steps have new
   committed intent identities, not retries of an uncertain step.
   Locked reqwest **0.13.5** defaults also retry protocol NACKs and follow
   redirects; bypassing Provider::complete's loop alone is insufficient.
   Use its actual `.retry(reqwest::retry::never())`,
   `.redirect(reqwest::redirect::Policy::none())` and `.no_proxy()` in the
   private native client. Fail client construction rather than falling back to
   a default client. Keep public/legacy Provider behavior intact.
4. **Accept minimal real credential carrier, not an auth bypass.** Choose the
   bounded SecretBytes carrier rather than config-backed fixture-only auth.
   Maximum 16 KiB, non-Clone/non-serde, redacted Debug, explicit clear/Drop and
   crate-private checked access only. Reject empty/oversize/invalid HTTP bearer
   values without including bytes in errors. Preserve public error shapes.
   Resolve exactly options.target; reject mismatched Native results. Never
   serialize auth/header/credential into request/status/intent/result evidence.
   Interactive secret inbox/reply/prompt, watch auth and external sinks remain
   Unsupported. Existing Provider String copies preclude memory-secrecy claims.
5. **Accept explicit temporary unsupported matrix.** Only the new private path
   rejects configured MCP/JEV and all child/model-lookup/override/Undo operations
   before process/network/metadata effects. No Subagents constructor or startup
   /models call. Legacy four-child/MCP/JEV behavior must not regress.
6. **Accept private fixture delivery only in B2a.** Retain exact authoritative
   completion/history durably and use a finite non-dropping critical collector
   for tests. Declare its item/body/aggregate ceilings and saturation behavior
   in the contract. Stream previews may remain disabled; never present a
   truncated/dropped delta as final content. Public delivery/activation needs
   a separately frozen minimal B3 subset. No lossy try_send bridge.
7. **Accept immutable retained status/run/intent lineages.** Keep all version-1
   envelope/acceptance golden bytes and tombstones. Latest reachable status must
   remain queryable after active removal/reopen. No mutable command object,
   unbounded historical map or independent sidecar authority. Validate all new
   lineage/membership/state transitions read-only before any recovery/write.
8. **Accept separate queue revision.** Initialize it from B1's canonical revision
   at migration. Membership/order/pause/hold/eligibility changes advance it;
   unrelated execution/evidence checkpoints do not. Snapshot/status revisions
   remain canonical revisions. Freeze overflow and migration behavior.
9. **Accept keyed live restrictions and dispatch fencing.** B2a must install/
   consume accepted Send restrictions by command identity; B2b owns cancel/
   replacement. Reconstruct validated keys on reopen. Do not alter legacy
   string-key consumption. Recheck ownership/generation/cancel/approval/policy
   after barriers and at concrete invocation. No mutex over fsync/await/scan.
   Crossing the gate before later stop is possible partial effect, not rollback.
10. **Accept exact native task authority.** Refresh filesystem evidence, then
    validate turn/task/contract/work and preserve WorkClock/assessment semantics.
    Model output/provider completion/tool success is not acceptance. B2a
    commits execution observations; task command/permit activation stays B2b/c.
    The exact contract must distinguish original protected task turn from the
    newly assigned execution turn when native follow-ups continue a task.
11. **Accept independent live controls.** One supervisor schedules, one leased
    store worker writes, one existing Session computes. Stop/owned approval
    intake bypass storage/delivery backpressure. StopAck requests cancellation,
    not cleanup/Finished/history_saved. Shutdown cannot fabricate durable drain
    or lease release while storage is blocked. State exact bounded failure and
    ownership behavior before implementation.
12. **Accept distinct later continuation/delivery rules.** B2a cannot activate
    ContinueTask, ResumeQueue, safe-boundary or StopThenSend. Freeze their
    eventual idle safe-boundary policy as an ordinary new eligible turn with
    no fictional step; injection binds the containing turn. Explicit resume/
    one-use consent never clears uncertainty, certifies a task or replays a
    Started identity. Full later gates remain in the prepared ENGINE-02/03.

## Required exact contract before code

The same owner writes only run-root `specs/backend-b2a-contract-proposal.md`.
The primary will review and transfer/freeze the approved contract into docs.
No implementation, tests, build-slot acquisition, provider/tool/native launch,
manifest/lock change or alteration of the completed PHASE 0 report.

Specify one concrete representation, not alternatives:

- Complete Rust declarations and serialized version/kind/field shapes for the
  v2 state extension, active run, status/run/intent/result references and nodes,
  private construction bundles, PreparedNativeTurn, NativeMutation, CommitAck,
  checkpoint client, live registry and bounded fixture collector.
- Exact valid transitions and lease/fence/queue/status/sequence/turn allocation
  rules for Start, Prepare, Dispatch, Complete, Checkpoint and Finish. Preserve
  concurrent newly accepted commands/unknown document fields. State whether
  Started remains active and how terminal removal retains its status.
- Exact read-only cross-lineage validation, size/identity/digest/revision/
  predecessor/membership checks, bounded-memory lookup and corruption no-write
  behavior. Legacy/B1 migration must reject reserved-key conflicts and preserve
  canonical envelope bytes; no automatic start on migration/reopen.
- Concrete scope/root/queue/intent/node/request/result/collector bounds and
  overflow behavior. Retain 20 commands/32 MiB queue, 32 inbox, 64 MiB snapshot,
  64 KiB lineage node, 100 rows/256 KiB page; at most one executing root and
  32 active intents. Body limits must accommodate existing 8 MiB completion
  and 16 MiB incoming-stream ceilings while preventing unbounded new state.
  Post-serialization checks are not preallocation/aggregate-memory guarantees.
- Exact staged preparation/native-loop persistence hooks, guarded built-in
  pre-spawn/pre-file-mutation/pre-fetch-send points, lossless failure unwind,
  single-attempt HTTP/client construction and finite live stop/approval routing.
  No earlier provider budget query, JEV, child or hidden legacy saver.
- Credential validation/ownership/clearing/error behavior. Check native error/
  Debug/receipt/log paths with dummy credential sentinels, including provider
  error-body echo. Explicitly identify any remaining privacy gap before code.
- Preserve the completed proposal's A1–A11 actual-store/native-provider/tool/
  approval/fault/race/recovery/bound/regression gates. Add transport-policy
  verification and redirect-destination zero-request evidence; plain one-call
  unit counters do not prove disabled reqwest transport retries/redirects.
- Exact owned files per B2a only, test-only activation mechanism and how strict
  production/test builds avoid new warning allowances. No public bypass/API
  addition merely to make private fixtures work. No B2b/c, ACP/app/renderer,
  dependency/root-lock or filesystem guard changes without another ruling.

List any remaining concrete disagreement. Do not quietly implement an unknown
schema, weaken a gate or claim the private seam is connected public execution.
After contract approval the same worker may implement the bounded B2a slice;
that later authorization is not supplied by this document.
