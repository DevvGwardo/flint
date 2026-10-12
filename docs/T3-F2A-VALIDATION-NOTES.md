# F2a remaining validation preparation

Status: primary inspection notes only. No implementation, toolkit/root edit,
build or test is authorized by this document. The unchanged frozen F2a and
native-selection/action-review gates remain authoritative.

## Stage filesystem-effect evidence

The inspected Stage path calls the retained ordinary input's `insert`, followed
by notification. The existing composer Change subscriber routes to
`composer_changed`, not submit or steer. Its mention/slash handling is separate
from engine dispatch. This is source inspection, not a runtime filesystem pass.

The existing actual-view Stage test compares one image attachment's bytes,
canonical rows, draft/attachment/input identity, Undo and empty engine intake.
It does not inventory the fixture trees. Extend the same real-view test scope
with the following evidence once the existing frontend owner is authorized:

- Use only the fixture-owned app home and workspace from `test_options`.
  Initialize synthetic files/attachments and settle all fixture events before
  taking the baseline. Never inspect the user's home/workspace/configuration.
- Compare the complete bounded fixture tree before/after the real Stage button,
  including path sets, regular-file contents and sizes, modes, identities,
  modification/change metadata and symbolic-link targets without following
  links. Keep the previously opened file handle for an important immutable
  history/queue sentinel, as the existing task-boundary test does, so an atomic
  replacement cannot be mistaken for unchanged bytes.
- Drain ordinary UI/input/observer work after activation before checking. Do not
  count setup writes, pending scripted engine writes, or observation-induced
  access times as effects of Stage. Do not exclude a changed app/queue/history
  file merely to obtain a pass.
- Retain the real clipboard/draft/attachments/Undo, canonical transcript and
  zero-engine-operation checks. Include code that resembles a file/shell action
  or mention; it remains literal safely fenced draft content.

A fixture-tree comparison establishes only the measured owned-tree nonmutation
and inspected dispatch path. It is not a process-wide syscall audit or proof
that arbitrary system caches never write. Report that limit explicitly.

## Existing APIs and ownership

- `app.rs:352–369`: composer event subscription separates PressEnter and Change.
- `app_input.rs:46–71`: Change updates menus and notifies the UI.
- `task_review_boundaries.rs:54–75`: existing Unix metadata/open-handle pattern.
- `blueprint_ui.rs:1008–1020`: isolated fixture homes/workspaces and dummy keys.
- `docking.rs:372–380`: pane-local width routing was already present in the
  preserved dirty source. It is not an omitted new F2a edit.

Do not silently weaken selected-transcript, physical resize, manual-tail/focus,
restart, stale-owner/append, nested/incomplete-fence or narrow-child gates.
The frontend proposal is still running; any implementation waits for its
completed report and separate frozen native-slice ruling.
