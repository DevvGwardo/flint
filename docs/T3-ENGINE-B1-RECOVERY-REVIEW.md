# B1 recovery referential-integrity repair

Status: independent source finding, repair required before B1 acceptance.
This is an additional frozen repair gate, not permission for a concurrent
backend worker or a change to original B1 gates.

## Concrete source case

After one valid accepted Send, a damaged canonical snapshot can retain its
pending entry and immutable command/acceptance objects while setting
`orchestration.receipt_head = null` and `orchestration.receipt_count = 0`.
The current state validation checks head/count agreement and each pending
reference separately, but does not establish that a pending acceptance belongs
to the canonical receipt chain.

The current reopen path can therefore pass the separate object checks and
commit a new recovery revision over that damaged document. A queue query still
contains the command, while receipt lookup traverses an empty chain and returns
None. This contradicts referential integrity and same-ID reconciliation.
The finding is from source inspection; no passing/failing reproduction is
claimed until the regression runs.

## Required implementation and tests

Validate bounded pending entries against the canonical receipt lineage before
recovery rewrites any bytes. Cheap count/head checks are necessary, not
sufficient: a valid orphan acceptance or another valid chain branch must not
be accepted as canonical merely because its individual object hash is valid.
Validate matching acceptance-node identity, command object/digest and accepted
sequence/revision against the referenced canonical node.

Traverse one bounded receipt node at a time or use a verified equivalent.
Do not load historical captured payloads or construct an unbounded receipt map.
Keep the twenty-entry active working-set and existing page limits. Document
any linear metadata traversal; do not claim constant-time reopen.

Add actual-store regressions for:

1. Pending acceptance with missing canonical head/count.
2. Pending acceptance that is an existing valid object but not reachable from
   the canonical head.
3. Intact accepted/reopened receipts and existing long-history bounded-query
   behavior.

Damaged cases must return CorruptState before a recovery commit, leave
`history.json` and immutable objects byte-identical, and neither relocate
history nor invent/downgrade accepted work. Retain the failing-before evidence
where feasible and the subsequent focused/full checks.

No new dependency, root-lock change, legacy shape change, scheduler, provider
execution, warning suppression or broad refactor. Run through the existing
isolated shared slot. Extend the separate B1 repair report with actual facts.
The existing backend worker resumes this gate only after its current repair
invocation finishes; do not interrupt it or create a duplicate writer.
