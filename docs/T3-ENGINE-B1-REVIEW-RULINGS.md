# B1 independent review repair rulings

Status: targeted B1 repair authorized; B1 is not yet accepted.
Existing acceptance gates and legacy public shapes remain unchanged.
This additive contract correction is frozen before repair implementation.

## Required repairs

1. Remove the newly added `clippy::large_enum_variant` allowances from the new
   `EngineEvent` and `ResolvedTarget` declarations. Suppressing a newly
   introduced warning is not a strict-lint fix. These are new design-contract
   types, not pre-existing legacy wire enums.
2. Make version-1 canonical command encoding explicitly sort every JSON object,
   independent of Cargo feature unification enabling serde_json
   `preserve_order`. The installed serde_json 1.0.149 has the public
   `Value::sort_all_objects` method. Preserve the existing lexicographically
   sorted version-1 bytes; do not change the digest algorithm/version or rewrite
   accepted objects.

## Exact additive Rust contract correction

This supersedes only the corresponding planned declarations in sections 4 and
6 of `T3-CONTRACTS.md`; the original frozen baseline is retained unchanged.

```rust
pub enum EngineEvent {
    Record(Box<EngineRecord>),
    Progress(ProgressUpdate),
}

pub enum ResolvedTarget {
    Native(Box<ResolvedNative>),
    External,
}
```

Box is transparent to serde; no legacy `Op`, `AgentEvent`, `AgentConfig` or
`SessionHandle` type, variant or serialized representation changes. Other new
contracts remain as frozen. App integration must use this corrected shape.
No broad refactor, new dependency, root-lock change, scheduling, provider
execution, warning allowance or gate adjustment is authorized.

## Validation and evidence

- Add a canonical-byte/digest regression with nested objects and deliberately
  nonlexicographic struct-field order. Retain a version-1 golden envelope so
  future feature changes cannot silently alter command identity.
- Exercise same-ID retry/reopen against the existing version-1 object bytes.
- Run focused orchestration tests, full agent/ACP regressions, strict
  affected-crate all-target Clippy and owned-file formatting under the existing
  isolated shared build slot. No warnings are suppressed.
- Preserve earlier failures and original B1 raw report; write the repair
  commands/counts, exact corrected signatures and source hashes separately to
  run-root `specs/backend-b1-repair-results.md`.

The primary independently reruns gates and reviews source before B1 acceptance.
B2 execution remains unauthorized.
