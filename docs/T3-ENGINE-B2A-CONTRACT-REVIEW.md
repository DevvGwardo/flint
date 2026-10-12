# B2a exact-contract review: corrections before implementation

## Status

The same backend owner's completed proposal is run-root
`specs/backend-b2a-contract-proposal.md`. The primary read all 1,385 lines and
checked the relevant existing store, persistence, task, Session and approval
seams. This is source/contract review, not a test or implementation result.

Do not freeze or implement the full proposed schema as-is. Return the
corrections below in one **new** addendum:

`/Volumes/T7 Shield/mac-offload/flint-t3-implementation-glct94hg/specs/backend-b2a-contract-corrections.md`

Owner remains `ce806277-29c1-4906-be67-f7a4b0486a50`. Proposal-only work can
proceed while frontend N0 owns the source/root/tooling lease. No backend
source change, build, test, provider/tool effect or public activation now.
Preserve the original proposal and every prior frozen gate/result.

## Dispositions on the three stated limitations

1. **Accept the scoped memory statement.** Exact storage/admission/buffer
   ceilings do not prove a hard overall heap/RSS ceiling for unchanged
   provider parsing, decoded history, diff scratch, TLS or kernel allocations.
   Keep the 16/8/1 MiB provider ceilings and all explicit new limits. Do not
   relabel postallocation checks or encoded byte permits as a hard process
   bound. A later hard aggregate guarantee requires its own reviewed scope.
2. **Accept truthful storage uncertainty.** Successful commits must retain
   exact outputs. Disk-full/representation failure cannot guarantee a durable
   exact result. Prohibit further effects and terminal success, retain available
   live evidence, preserve Dispatching/uncertainty, and never truncate/retry/
   invent rollback. This is not permission to drop successfully committed
   critical references or to claim shutdown saved a failed result.
3. **Accept scoped host-auth protection only.** Safe private errors must never
   echo response bodies/SSE errors/reqwest detail. Keep the minimal real
   redacted/clearing carrier and exact known-token guards. No claim of arbitrary
   encoded-secret confinement, full memory erasure or filtering starting user
   content/legacy Provider errors. These limits do not authorize secret values
   in the new path's chat, nodes, notices, Debug or test evidence.

## Numbered corrections

### R1. Same-transaction predecessor revisions

Proposal section 4 says multiple nodes in one canonical transaction share the
same revision. Section 8 step 3 requires each predecessor to have a smaller
sequence **and revision**. Recovery/migration appends multiple command-status
and intent transitions in one transaction, so both rules cannot hold.

Correct the exact validator rule: global predecessor sequence is strictly
smaller, ordinal decreases by one, and revision is **nonincreasing** when
walking backwards. Freeze the additional same-revision transaction grouping/
per-entity uniqueness rules so an arbitrary duplicate transition cannot hide
in that relaxation. Keep run_revision and execution fences distinct.

Require an actual-store future regression with at least three changed pending
receipts and two intent transitions committed in the same recovery transaction,
then read-only validation/reopen/latest queries. Also reject backwards
revision growth, duplicate sequence and illegal same-entity same-revision
transitions. B1 acceptance bytes remain unchanged.

### R2. Exact v1/v2 store and stopped-task seam

The proposal declares StateV2 but does not freeze the version-discriminated
Store representation and exact v2 submit/queue/summary/stopped-task behavior.
Existing Store owns `state: State` (version 1), and its queue exposes canonical
revision. Existing `persist::save_task_locked` changes only task and canonical
revision (`persist.rs:287–307`).

The proposed validator expressly permits a stopped-task edit/review to make
the current task differ from historical run projections. Yet ownership lists
`persist.rs` only for pure restoration repair and leaves the v2 mutation/
queue-eligibility accounting unspecified.

Provide exact Rust declarations for the selected v1/v2 representation and
common accessors, preserving ordinary B1 opens/serialization. Specify which
existing function validates and commits v2 stopped-task changes under the
same lease, how queue_revision advances for a real eligibility change, how
current holds/task revisions are projected without auto-resume, and how
historical results/terminal receipts remain immutable. No second Saver or
public scheduling route. Explicitly request only the necessary additional
persistence seam ownership, not a general persistence rewrite.

Future tests must cover stopped edit/review on v1 and v2, correct separate
queue/canonical revisions, retained extensions/history/tombstones, no
resurrection of Started recovery-held work, and corrupt v2 rejecting before
canonical/object mutation.

### R3. Preflight before immutable publication

Section 4 requires preflight of every allocation and serialized limit before
any immutable publication. Section 7 describes a chunk writer that flushes
parts as serialization progresses. Without a prior size/shape pass, an
oversized later result or merged snapshot is discovered only after parts have
already been published.

Select an explicit bounded deterministic two-pass contract for newly encoded
structured bodies: capped measurement/validation/digest/part-count and final
merged-snapshot preflight first, then streaming publication from the same
owned immutable values. Define exact byte equivalence checks and charged
scratch/snapshot/request lifetimes. Do not allocate a giant encoded result,
clone the full projection or hold the live-control mutex over either pass/
publication. Exact already-frozen HTTP bytes remain single-buffer authority.

Storage I/O failures after preflight may still leave nonauthoritative orphans.
Representation/size failures found in preflight must publish zero new
immutable objects. Add future boundary/Unicode-escape/snapshot-extension
fixtures that inspect the complete object directory before/after rejection,
plus byte-equality and publication-fault cases.

### R4. Keep the next implementation reviewable

The complete private executor, three lineages, recovery, giant-body streaming
JSON lexer/duplicate-key scanner, live authority, provider refactor and tools
are not one small typing slice. A detailed 98 KiB proposal does not remove
that implementation/verification risk.

Choose **B2a-0 immutable blob codec** as the first proposed implementation
stage. Give its complete small contract in the addendum: checked ObjectName/
ObjectRef/BlobRef/BlobKind/BlobManifest; 1 MiB parts, 16 KiB manifests, at most
128 parts; exact lengths/order/digests/zero-body rules; anchored no-follow
readers; bounded streaming raw-body visitor; preflight/publication behavior;
all overflow/corruption/unknown-field/duplicate-key outcomes.

Keep this first stage's new module and tests test-only until a separately
reviewed production v2 consumer exists. Do not activate StateV2, rewrite B1
history, implement the historical giant-body JSON scanner, change native
Session/provider/tools/secrets, or add dead-code/lint allowances in B2a-0.
Use actual Directory/Store fixtures and existing immutable storage primitives.
No new dependency or filesystem guard change.

Specify one exact owned module/test registration and raw test commands.
Required evidence includes exact multi-part roundtrip (including repeated
part hashes), empty/boundary bodies, missing/corrupt/wrong-size/nonregular/
symlink objects, changed order/total digest, manifest bounds/duplicate keys,
preflight zero-publication and fault-created orphan nonauthority, bounded
scratch and unchanged B1 normal/preserve_order regressions.

The later full v2 codec/lineage stages still need explicit traversal-work and
cancellation behavior. Repeated scans may bound memory, but O(N²) key/history
work is not a bounded latency/CPU claim. Do not advertise the proposed
historical scanner as tested, complete or safe merely because scratch is
finite.

## Next checkpoint

Return exact corrections/stage contract and any new substantive disagreements
only. The primary will freeze the small selected stage after review and source
slot coordination. No backend implementation is authorized by this document,
and no B2a/private fixture result counts as connected public orchestration.
