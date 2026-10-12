# F2a paused-source action review

Status: independently reviewed source, not F2a acceptance. These corrections
queue for the existing frontend owner after its native-selection PHASE 0
returns. Do not interrupt that proposal, start another app worker, edit the
installed toolkit, or weaken the frozen F2a selection/ownership gates.

## Verified source findings

1. List-nested fenced blocks lose Copy/Stage. `chat_code.rs:171–184` takes the
   opening line's exact prefix and removes that same prefix from every body
   line. For the locked toolkit's own example
   `- ```rust\n  one\n  two\n  ````, the opening prefix is `- `, whereas the
   continuation prefix is two spaces. The reconstructed value keeps those
   spaces and fails the comparison with native `CodeBlock::code()`, which is
   `one\ntwo`. `code_action` returns None and the real buttons become disabled.
   GPUI Base 0.7.0's `text/format/markdown.rs:1507–1511` explicitly supports
   this example and its source ranges. This is a source-derived counterexample,
   not a newly executed mounted-view test.
2. Retained code receipts still deep-copy their source on unchanged frames.
   `CodeAction::source` is a String with derived Clone. The hook clones the
   previous map value, the value inserted into the map, and the Copy closure's
   receipt (`chat_code.rs:46,55,62`). Each clone copies the entire fenced span.
   The reuse path also compares the full canonical span each frame. This
   undermines the retained-row design and the implementation's no unbounded
   detail-copying gate. No allocation benchmark or whole-app speed claim is
   made from this inspection.
3. Hidden-owner deferred Copy is not covered by the existing stale-owner test.
   At `chat_readability.rs:138–141`, only Stage is attempted after changing
   owners. `copy_code` currently checks row identity/canonical bytes only, so
   an unchanged saved receipt from a hidden root remains accepted. Preserve
   legitimate deliberate Copy from a currently visible tiled pane; do not
   simply impose the active root's identity on every pane.

The cache clears stored receipts on row revision changes and releases their
maps with successful owner retirement. Stage rejects selected children and
other selected roots before changing the ordinary composer. Those inspected
paths do not erase the three findings above.

## Queued same-owner correction gates

- Keep native parsed CodeBlock/source-span verification. Correct list,
  blockquote, combined nesting and continuation indentation using the actual
  locked parser semantics, not a permissive substring search or regex fence
  removal. Incomplete or stale asynchronous parses cannot mint action receipts.
- Retain immutable shared source/receipt data, or an equivalent bounded
  revision-bound design. Unchanged renders must not clone or rescan every full
  fenced span. Revalidate canonical ownership immediately before a deliberate
  clipboard/draft effect. Keep selection/streaming/cache retirement semantics.
- Prove deferred Copy from a hidden/removed owner and stale append/replacement
  cannot change the clipboard. Keep actual visible tiled/child Copy usable and
  Stage's explicit ordinary-draft/read-only routing. Use genuine native-hook
  receipts, including a failing-before hidden-owner case.
- Add mounted-view actions for the exact list example, ordered/combined nesting,
  blank lines, Unicode, CRLF, longer fences and incomplete-to-complete streaming.
  Copy must match the locked parser's exact code value; Stage must produce an
  ordinary safe fenced draft. Retain current exact-code/trailing-newline tests.
- Finish the already-required selected-transcript Copy/Stage, physical resize,
  manual-tail/focus/restart and isolated no-filesystem-effect evidence. Run
  relevant F1/F2a/app regressions, owned formatting and strict affected Clippy
  through the shared isolated slot. Retain failures.

Implementation still needs the separate native-selection proposal ruling and
explicit ownership/root-dependency coordination. This document authorizes no
vendoring, manifest/lock edit, highlighting, renderer launch, provider operation,
user-app change, commit or publication.
