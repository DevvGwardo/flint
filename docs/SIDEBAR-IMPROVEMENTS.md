# Sidebar improvements: usability findings

Read-only review of the sessions sidebar. No code was changed, no tests were run;
every claim below comes from reading the source.

Status 2026-10-17: items 2 (⌘F), 3 (collapsible headings), 4 (archived
disclosure), 7 (grouping menu) and 8 (quiet ellipsis) shipped with
`docs/PLAN-SIDEBAR-SIMPLIFY.md`; its §5 also records what was deferred. Items 1
(roving focus), 5 (auto-extend the page) and 6 (back to current session) remain
open.

Scope: `crates/flint-app/src/sidebar.rs` (1120 lines), with grouping in
`crates/flint-app/src/session_groups.rs` and key bindings in
`crates/flint-app/src/lib.rs`.

## What the sidebar is today

- `render` (sidebar.rs:105–155): brand mark, panel grip and close control.
- Three fixed nav rows (157–186): New agent, Open folder, Terminal.
- Search `Input` (188–198).
- Four filter chips laid out as two rows of two (341–417, `filter_chip` 665–712).
- The scrolling grouped session list (464–511).
- An always-visible archived block capped at 112px (512–621).
- Bottom bar (418–441): Settings plus a fixed 98px grouping cycler.

Grouping is computed in `session_groups.rs`; paging is `SESSION_PAGE` in
`app_actions.rs:161`.

## Where it is hard to use

1. **No list keyboard navigation.** Every session row is a tab stop
   (`tab_index(0)`, sidebar.rs:895) and every row's ellipsis is another
   (`session-actions`, 1016), so tabbing through 20 sessions is 40+ stops.
   There is no roving focus and no Up/Down handling in `app_input.rs` or
   `layout.rs`.
2. **Search has no shortcut.** `bind_keys` (lib.rs:124–158) binds ⌘N/⌘K/⌘J/⌘B/
   ⌘⇧G/⌘O/⌘L/⌘, and more, but nothing focuses the session search; ⌘F is free.
   The escape chain in `layout.rs:150–234` unwinds menus, rename and the drawer
   but never clears the search, so a search that hides everything needs a mouse.
3. **Groups cannot be collapsed.** The heading at sidebar.rs:240–305 is a plain
   `div` with a label and counts: no role, no click, no chevron. `GroupKey` and
   `SessionGroups` already carry everything needed to key collapsed state. With
   several projects the list becomes one long undifferentiated scroll.
4. **Archived is permanently expanded.** sidebar.rs:512–621 always renders up to
   112px of archives when any exist, taking list height for something opened
   rarely, with no disclosure control.
5. **"Show more" is mouse-only and manual.** `show_more` (626–662) is a click
   target; nothing extends the page when the list reaches the bottom, so a
   100-session workspace means repeated clicking.
6. **The active session can be scrolled out of view** with no affordance to jump
   back. `reveal_row` (79–103) only fires on focus changes.
7. **The filter strip reads as a form.** Two rows of two with reserved 30px count
   slots (692–711). The reserved slot is needed to stop counts moving targets
   (fixture `sidebar_filter_positions_survive_multi_digit_counts`), but the 2×2
   split doubles the strip height for four values, and empty chips are only
   dropped from the tab order (679), not from view.
8. **Grouping is discoverable only by tooltip.** The 98px control (372–404)
   truncates its label and cycles rather than listing choices; ⌘⇧G exists but is
   invisible in the UI.
9. **Per-row noise.** Rename and Archive live only in a right-click menu
   (915–921) or the always-visible ellipsis, whose glyph is painted on every row
   at full opacity, competing with the status glyph that carries the meaning.

## Suggested work

No restyling: same palette, sizes, radii and spacing tokens.

Ordered by value per line of code:

1. Roving focus in the session list: one tab stop for the list, Up/Down/Home/End
   move the selection, Enter opens, Shift+F10 or Right opens the row menu,
   reusing `reveal_row` so off-screen rows scroll into view.
2. ⌘F focuses the session search, Escape clears it when it has text (added to the
   `layout.rs` escape chain before the drawer step), Enter selects the first
   visible match. Update `shortcuts.rs` labels and the README key table.
3. Collapsible group headings: give the heading a Button role and a chevron from
   the existing `ChevronDown`/`ChevronRight` icons, and persist collapsed
   `GroupKey`s in `Settings` beside `session_grouping` (settings.rs:154).
4. Turn Archived into a disclosure: the header row toggles the list, collapsed by
   default when the session list is long, remembered in `Settings`.
5. Auto-extend the page when `sidebar_scroll` reaches `max_offset()`, keeping
   `show_more` for discoverability.
6. A "back to current session" row shown only when the active row is outside the
   viewport.
7. Make grouping a real control: a menu with Project/Status/Agent and a checkmark,
   instead of a cycling button.
8. Hide the ellipsis glyph until row hover, focus or active, keeping its 24px slot
   so bounds-based fixtures such as
   `sidebar_timestamp_does_not_take_space_from_the_title` still hold.

Items 1, 2 and 3 change how the sidebar feels; 4–8 are polish. Items 1–3 touch
`sidebar.rs`, with 2 also needing `layout.rs`, `lib.rs`, `shortcuts.rs` and
`settings.rs`, plus new fixtures under
`crates/flint-app/tests/blueprint_ui/sidebar.rs`.

## Verification

None. No build, test, clippy or formatter run was performed for this note, and
nothing here is verified against a running app.
