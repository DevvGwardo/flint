# T3 chat slice 02: readability width and safe code actions

Status: prepared gates only. Do not start until F1 passes independent review
and its focused/lint gates. This does not authorize a concurrent app worker.

## Scope and ownership

Frontend owns `crates/flint-app/**`, preserving the retained managed text,
virtualized lists, resource denial, drafts, task review and queue authority.
No engine/protocol/root manifest/lock or renderer-helper edits in this slice.
No new dependencies, real providers, native-app activation or code execution.

This is F2a. Real syntax highlighting is F2b with separately coordinated GPUI
feature/dependency acquisition. Plain text or a language label is not a claim
that syntax highlighting has been implemented.

## Design contract

1. Persist a global transcript-width preference: Readable 760 px, Wide 1040 px,
   or Fill available pane. Readable is the compatible default. Malformed/unknown
   width values fall back locally without erasing unrelated settings.
2. Expose an accessible real settings/control entry point. Parent, child and
   tiled transcripts use their own available pane bounds, not the active
   conversation's width or the entire window width. Clamp to available space
   after padding; composer and transcript agree. Welcome width stays separately
   intentional.
3. Resizing or changing width remeasures/reflows without replacing retained text
   entities, resetting selection, changing tail-follow choice or stealing focus.
4. Fenced code uses the real GPUI code-block action hook. Show the reported
   language as a label, unknown languages as readable monospace.
5. Copy copies the exact code source on deliberate activation. Stage inserts a
   correctly fenced code snippet into an ordinary draft, never sends or runs it.
   Preserve the existing draft and attachments. Verify actual GPUI input APIs
   rather than replacing input state unnecessarily.
6. Actions retain stable conversation/row ownership. A child is still read-only
   before F5: staging must either be visibly unsupported with a reason, or be an
   explicitly named user action to the parent's composer with visible navigation.
   Never silently write a hidden parent draft or another selected conversation.
7. Code headers/actions stay usable in 320/420 px panes. Long code scrolls inside
   its block, not by widening the transcript/composer or hiding actions.
8. Do not add Run/Apply/Open-shell buttons, interpret a fence language as a
   command, or relax resource/approval policy.

## Frozen acceptance gates

- Width setting survives save/restart, old settings retain Readable default, and
  invalid width does not reset existing endpoint/permission/theme settings.
- Actual settings/control interaction changes painted parent/child widths.
- At 320/420 px pane widths within a much wider tiled window, code/header/actions
  and composer remain within pane bounds and scroll correctly.
- Actual Copy button changes the clipboard to exact multi-byte/arbitrary-fence
  code. Actual Stage action preserves existing draft/attachments and does not
  dispatch an engine operation or perform a filesystem effect.
- Retained selection/entity identity and manual tail pause survive width changes,
  code action activation and conversation switches.
- Deferred/stale code actions cannot affect another conversation or a changed row.
- Relevant settings, selection, streaming, subagent, tiled-pane and composer
  regressions pass; add real `FlintApp` interaction tests, not only width math.
- Formatting and strict affected-app Clippy pass under the isolated serial slot,
  with failures retained and no warning suppression.

## Validation/reporting

Use run-root `specs/validate.py`, offline locked commands, fresh homes and the
pass-owned build/native slot. Format owned files only while engine work is live.
Return exact touched paths, commands/exits/counts and limitations in run-root
`specs/frontend-f2a-results.md`. Do not grade the slice or silently count pending
F2b highlighting as completed.
