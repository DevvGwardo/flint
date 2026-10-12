# T3 chat slice 01: retained Markdown and resource safety

Status: approved for implementation. Gates below are frozen before results.

## Authority and ownership

This slice belongs to the frontend Factory worker. Preserve the dirty starting
source, native GPUI design, existing list virtualization, task outcomes, queue
authority, and the running user application. See `T3-IMPLEMENTATION.md`.

Edit only `crates/flint-app/**`. Do not edit shared engine/protocol crates, the
root manifest/lock, historical contracts, or run feature providers. Shared
backend types remain unchanged during this slice. No new dependencies are
needed. The architect owns documentation and rulings.

## Rulings on frontend PHASE 0

1. Accept existing virtualization and batched folding as the foundation.
2. Accept retained `Entity<TextViewState>` with incremental append, not a new
   parser or a rebuilt transcript.
3. Accept extending the existing draft map in a later slice, not duplicating it.
4. Accept preserving queue edit/steer and task holds until deliberate migration.
5. Accept that native HTML is not a browser. Final fidelity is a separate
   architecture decision, not a claim this slice can make.
6. Accept explicit resource policy for Markdown and HTML before rendering.
7. Accept separate source/rendered UTF-8 ranges. Never interchange them.
8. Accept explicit rich-render/cache budgets, without an unmeasured speed claim.
9. Accept parent-owned child control, pending backend capabilities.
10. Accept lazy hydration only with canonical task-recovery reconciliation.
11. Accept non-destructive history forks, not file-restoring Undo.
12. Accept masked inputs without claiming allocator or process-memory security.

## Implementation contract

- Add app-local chat-content/resource-policy modules. GPUI entities belong to
  the app/session, not the pure canonical event/view projection.
- Row ownership uses conversation identity and a stable monotonic row identity
  assigned during ordered folding, independent of token batch boundaries or
  row index. Child IDs cannot collide with parent IDs.
- Streaming-to-final-answer promotion retains the same text entity and
  selection. Trusted append revisions append only new slices; explicit
  replacement/replay revisions replace once.
- Create offscreen retained text only on demand. Synchronize touched retained
  rows after an event batch, not a full-history prefix comparison each frame.
- Keep virtual-list splice/remeasure and follow/pause/jump behavior intact.
- Retain at most 128 non-pinned text entries and 8 MiB non-pinned source.
  Rich parsing stops at 512 KiB per row. Show an explicit bounded fallback,
  preserve canonical text, and make pinned exceptions/counts inspectable.
- Intercept images, embedded HTML, and resource references before GPUI can
  load a renderer-selected path or URL. Use actual parser hooks, not regex
  stripping that can be bypassed by HTML/entity/reference forms.
- Preserve user-activated ordinary workspace file previews through a
  workspace-aware validated handler. No automatic file/resource reads.
  Untrusted rich HTML will use a stricter, separate link policy.
- Reject credential-bearing external links and unsafe schemes. External
  navigation requires deliberate user activation.
- Reuse one stable parser/resource-policy configuration. Do not allocate a
  newly revisioned Markdown extension bundle on every render.

## Frozen acceptance gates

1. Real parent and child assistant/answer rows use retained managed text.
2. Appends, replacements, terminal promotion, replay, and conversation switches
   preserve the correct content and do not share entities across conversations.
3. Selection and Copy continue to operate on the actual rendered text.
4. Split UTF-8, partial fences/tables, event-batch boundaries, offscreen return,
   cache pruning, oversize fallback, and tail pause/jump have regressions.
5. Malicious Markdown images, reference images, embedded HTML resources,
   absolute/relative paths, unsafe schemes, and credential URLs cannot trigger
   implicit resource loading. Safe deliberate links retain their behavior.
6. Existing view-model/session/subagent/selection/transcript tests still pass.
   New behavior includes actual `FlintApp` headless interaction tests, not only
   detached parser/cache tests.
7. Formatting and affected focused tests pass. Retain raw failures and later
   fixes. No performance result is claimed from the implementation alone.

## Validation and reporting

Use only the run-owned serial validation slot:

`/Volumes/T7 Shield/mac-offload/flint-t3-implementation-glct94hg/specs/validate.py`

Use offline locked Cargo commands and disposable homes. Do not run another
build/native app, change the user's binary, or format another worker's files.
Report touched paths, exact commands, exits, counts, retained evidence paths,
and concrete remaining defects. Do not grade the slice.

## Out of scope

New shared backend enums, queue migration, private inputs, provider calls,
browser/full-Mermaid fidelity, lazy-history migration, and additional dependency
features. Those need their own frozen contracts and slices.
