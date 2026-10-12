# B1 event-layout and recovery repair continuation

Status: same backend worker authorized for remaining B1 repairs only.
No B2 execution and no duplicate backend writer.

## Observed lint result and exact contract correction

The prior repair removed both warning allowances, boxed Record/ResolvedNative,
and passed 28 orchestration tests in ordinary and preserve_order modes.
Full regressions passed: agent 273/1 ignored; ACP 74. Strict Clippy still exited
101 because unchanged ProgressUpdate is 232 bytes beside an eight-byte Record
box. This is an actual retained failure, not a passing lint gate.

Supersede only the new EngineEvent declaration from the original baseline and
first repair ruling with:

```rust
pub enum EngineEvent {
    Record(Box<EngineRecord>),
    Progress(Box<ProgressUpdate>),
}
```

ResolvedTarget remains `Native(Box<ResolvedNative>), External`. Boxed serde
payloads retain the same serialized representation. No legacy public type or
wire shape changes, padding tricks, threshold changes or warning suppression.
Keep the explicit recursive sorting and version-1 golden/reopen regressions.
The resolved serde_json version is 1.0.151; the verified sorting API also
exists in cached 1.0.149. Do not change versions or the root lock.

## Recovery gate

Implement the already frozen `T3-ENGINE-B1-RECOVERY-REVIEW.md` gate: pending
acceptance must belong to the canonical receipt lineage before a recovery
checkpoint can rewrite history.

Apply the same corruption rejection to stopped v3 task mutation preflight:
review/edit must not task-only rewrite a document whose pending acceptance is
missing from canonical lineage. Reuse the appropriate bounded validator; keep
read-only helpers free of directory creation or recovery writes.

Test missing head/count, valid orphan/branch acceptance, stopped mutation
refusal, intact retry/reopen and long historical receipt paging. Damaged
histories/objects stay byte-identical and are not relocated. No loading all
historical captured payloads or unbounded receipt map. Document any linear
metadata traversal.

## Ownership, validation and reporting

Only needed agent-owned orchestration/store/tests and existing persistence/task
preflight files. No app, ACP, renderer, frozen documentation, dependency/root
lock, scheduler/provider, approval or sandbox changes.

Under the isolated shared build slot, retain failing-before reproduction,
focused and preserve_order tests, full agent/ACP regressions, strict all-target
Clippy and owned-file formatting. A strict failure blocks acceptance; do not
ignore it or introduce an allowance.

Preserve both existing raw reports. Return run-root
`specs/backend-b1-final-repair-results.md` with actual files/hashes, corrected
signatures, commands/exits/counts and limitations, no verdict. Independent
primary validation and review remain prerequisites for B1 acceptance and B2.
