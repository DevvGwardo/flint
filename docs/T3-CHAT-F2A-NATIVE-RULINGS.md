# F2a native compatibility rulings and N0 baseline

## Status and prior gates

The same frontend owner's completed proposal is run-root
`specs/frontend-f2a-native-phase0.md`. These additive rulings do not change the
frozen F1/F2a selection, entity, source-range, tail, focus, action, resource or
authority gates. F2a remains incomplete.

Authorize **N0 provenance/tooling/locked baseline only** below. Do not yet
implement the native selection API, app annotations or queued action repairs.
N1 requires primary review of actual N0 results and a separate frozen scope.
This prevents an unresolved strict-native-lint/dependency prerequisite from
being hidden under a larger implementation.

Owner: existing frontend worker
`da6c5976-0e43-4814-b8dc-9e746b980e45`. No second native/frontend worker.

## Disagreements D1–D6

1. **D1, accept with staging.** Explicitly expand this owner's boundary to the
   two repository-owned locked native copies and narrowly leased root/tooling
   files. N0 is mechanical provenance and baseline work, not API activation.
2. **D2, accept.** Base cannot read inherited opacity through the installed
   public API. Approve the locked Pre source copy as a prerequisite. Its
   future single read-only drawing accessor is the selected approach; no
   Pre event/root/rendering policy change. N0 copies Pre without that change.
3. **D3, accept.** Exclude both native packages from workspace membership;
   use relative crates.io path patches. Do not resolve their development/
   bench graph, substitute upstream locks or add dependency/features.
4. **D4, accept.** Future policy registrations require a fresh completed
   native paint, live root marker, actual occlusion-aware hitbox and matching
   window/scope/epoch. Cap 256 and fail closed for the whole overflowed frame.
   A surviving marker/cached replay is not current visibility evidence.
5. **D5, accept as a separate mandatory gate.** Give this same owner the
   explicitly bounded runner/test lease below. Fingerprint every copied
   regular file and reject links/special/generated files. Selected native
   library diagnostics must be uncapped; app lint is not a substitute.
6. **D6, accept as future narrowly scoped work.** MouseUp Capture cleanup
   must survive a native handler stopping Bubble, without changing ordinary
   pointer/touch/keyboard dispatch. N0 does not edit this state machine.

## Required corrections before N1

- Proposal sections 2.1/2.6/7 must distinguish **Copy availability**, **Stage
  routing**, and **selection-preservation eligibility**. Keep deliberate Copy
  from a currently visible tiled or child owner usable. Reject deferred Copy
  from hidden/removed/stale owners. Stage retains its selected ordinary-root
  draft restriction and disabled child behavior. An inactive pane may be
  ineligible for a selection exemption without making its legitimate Copy
  action unavailable or changing its normal pane-activation path. Do not use
  a blanket active-root check for all three concepts.
- Root visibility must use the real computed native style, including hover,
  active, focus and semantic refinements, not only Button's resolved instance
  style. Installed Pre `Interactivity::compute_style` is public at
  `elements/div.rs:3383–3522`; inherited opacity is private at
  `window.rs:4120–4126`. Require finite positive effective opacity, actual
  content mask/viewport/hit test, and production operation without observers.
  Do not add a substitute hitbox or duplicate element/accessibility ancestry.
- Preserve native keyboard/accessibility focus and exactly-once activation.
  Do not disable focusability to pass the pointer-focus gate. Installed Pre
  `elements/div.rs:2765–2780` has a native default-focus path, distinct from
  selection policy; keep the existing consciously configured app behavior
  and test pointer versus explicit keyboard traversal separately.
- Keep all action findings in `T3-CHAT-F2A-ACTION-REVIEW.md` and Stage evidence
  in `T3-F2A-VALIDATION-NOTES.md`. No repeated full-span scan/deep copy on an
  unchanged frame, permissive fence substring search, fake touch/input,
  post-hoc range restoration or relaxed assertion.

## N0 exact ownership and source acquisition

Only the following new/leased paths may change:

1. `crates/flint-native-gpui-base/**`: original Base 0.7.0 archive contents,
   plus one `FLINT-PROVENANCE.md`; no upstream source modification in N0.
2. `crates/flint-native-gpui-pre/**`: original Pre 0.3.7 archive contents,
   plus one `FLINT-PROVENANCE.md`; no upstream source modification in N0.
3. Root `Cargo.toml`: add workspace exclusions for exactly those paths and
   `[patch.crates-io]` entries `gpui-base`/`gpui-pre` pointing to those relative
   paths. Preserve the four existing workspace members and all other content.
4. Root `Cargo.lock`: retain both package names/versions/dependency lists;
   remove only their existing registry source/checksum pairs. Preserve every
   other package and current dirty lock work.
5. Run-root `specs/validate.py` and `specs/validate_tests.py`: only full-copy
   fingerprinting/fail-closed provenance coverage and its mocked regressions.
   Keep existing command/environment/timeout/lock/contract/binary protections.
6. New run-owned provenance/baseline snapshots, logs and one raw report
   `specs/frontend-f2a-native-n0-results.md`. Do not rewrite old reports.

Read then snapshot current root/tooling bytes before editing. Stop if either
destination exists; never overwrite an untracked directory. Verify archives
against the current locked checksums before extraction:

| Package | Archive SHA-256 | Original regular files / bytes |
|---|---|---|
| gpui-base 0.7.0 | `64bb52b29a8dcdc8d3f595b8b0839c076f237572ab601ba4577d3da8ae085728` | 207 / 4,125,694 |
| gpui-pre 0.3.7 | `0e87a42bb37c7cb4e76dd1ac0ce88851e46e976e0373a47ab3e0757abffee54d` | 156 / 8,150,550 |

Use the existing cached archives, not a download or registry-source edit.
Validate safe package-relative paths, reject link/special/duplicate members
and verify every copied original byte before adding provenance. Exclude only
the cache's `.cargo-ok` marker, which is not an archive member. Preserve
normalized/original manifests, nested locks, licenses, attribution, resources
and build scripts. Nested locks are provenance, not resolver authority.
Retain full original and final relative-file hash manifests.

## Tooling and serial baseline gates

Expand `source_fingerprint` to include every regular file, including hidden
files/resources/attribution, under both exact native-copy roots. Reject symlink
roots/entries, special files and generated output under these roots without
following them. Keep prior source coverage intact. Test additions must prove:

- Add/change/remove a copied non-Rust resource, license, nested lock and
  provenance file changes the fingerprint.
- A copied resource mutation during a mocked command rejects stable-source
  acceptance, preserving before/after evidence.
- Symlink roots/files/directories and a special/generated entry fail before
  a real command could launch; absent not-yet-copied roots remain supported.
- Existing environment, frozen-contract/binary, timeout/cleanup and renderer
  fingerprint tests still pass. Mocked tooling tests launch no Cargo/native
  process and use only test-owned temporary trees.

Run all mocked tooling tests in a filtered environment under the shared slot,
with before/after whole-source, tooling, frozen-contract and binary brackets.
Then use the existing absolute run-root validation wrapper, labels beginning
`f2n-n0-`, timeout at most 1200 per command, and `--require-stable-source`:

```text
cargo check --offline --locked -p flint-app --lib -v
cargo clippy --offline --locked -p gpui-base --lib -v -- -D warnings
cargo clippy --offline --locked -p gpui-pre --lib -v -- -D warnings
```

Retain verbose selected package/source/diagnostic evidence. Verify one path
identity per package and absence of a native diagnostic cap. Do not infer
strict native lint from capped dependency warnings, zero compiled targets or
an app-only check.

If Cargo requests any further lock/dev-graph change, package selection fails,
native diagnostics are capped, or original upstream strict lint fails, stop
and report the exact raw prerequisite. Do not drop `--locked`, download,
regenerate the graph, add workspace membership, remove dev dependencies,
format/repair upstream source, add allowances or secretly broaden tooling.
The primary must freeze any necessary follow-up plan.

No app/agent/ACP/renderer edit, highlighting, native GUI/probe launch, provider
request, user-app activation, permission change, commit or publishing. This
N0 check compiles trusted existing dependencies only in the pass-owned target.
Keep all failures. N0 completion is not F2a acceptance.
