# B1 independent acceptance for bounded progression

Status: B1 durable acceptance/recovery foundation is accepted. This is not
provider execution, app migration, B2 implementation or final workspace/native
acceptance.

## Reviewed source and retained corrections

Independent source review cleared the new enum-size warning suppressions,
feature-sensitive canonical object ordering and missing canonical receipt-lineage
membership checks. Record and Progress payloads are boxed without changing
legacy event serialization. Version-1 JSON sorting is recursive.

Recovery/read-only/stopped-task preflight verifies the bounded reachable receipt
chain and pending acceptance fields before any mutation. Existing-child lookup
creates and syncs nothing. Ambiguous/uncertain recovery remains held. No provider,
scheduling, approval or task-certification authority was enabled by B1.

## Independent checks

- Focused orchestration tests: 37 passed, 0 failed.
- Focused tests with serde_json preserve_order: 37 passed, 0 failed.
- Strict agent/ACP all-target Clippy: exit 0.
- Final settled-source full agent/ACP command:
  `cargo test -p flint-agent -p flint-acp --offline --locked`.
  Cargo and isolated wrapper exits: 0. Agent units: 282 passed, 0 failed,
  1 ignored. ACP units: 74 passed, 0 failed. Additional guarded bench/eval/live
  harness tests: 3/5/1 passed with live/eval/benchmark opt-ins disabled; no real
  provider result is claimed.
- Whole-source fingerprint, frozen contracts and original release binaries
  remained unchanged throughout that final command.

Final full evidence:
`evidence/b1-independent-settled-full-1791261721874231000-result.json` and
matching log in the retained run root. Prior two passing-but-source-drifted
full runs remain failures of that gate, not retroactively accepted.

The earlier independent focused/mode/lint evidence and source review are
retained in `specs/backend-b1-review.md`. The original failure-before lineage
counterexamples and builder formatting/check results remain retained.

## Next authorization

The same backend owner may prepare **PHASE 0 only** for the native scheduling/
intent boundary in `T3-SLICE-ENGINE-02.md`, citing real executor/supervisor APIs,
numbered substantive disagreements, exact shared-type/integration ownership,
failure/ambiguous-effect recovery and offline fixture validation.

No B2 code, manifest/lock change, legacy protocol alteration or provider/tool
execution before a separate frozen B2 ruling. B3 delivery preparation remains
preparation only. Preserve task review/approval gates, the four-child native
executor and all starting user work.
