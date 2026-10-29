# Plan: a simpler, more professional sessions sidebar

Status: **plan only**. No source was changed, no build, test, clippy or formatter
run was performed for this document. Every height, id and line number below was
read out of the source; the pixel budgets are arithmetic over those values and
are marked `≈`, not measured on screen.

Scope: `crates/flint-app/src/sidebar.rs` (1120 lines), with grouping in
`session_groups.rs`, archive rows in the same file, subagent rows in
`subagent_ui.rs:267`, and the persisted knobs in `settings.rs`.

Companion reading: `docs/SIDEBAR-IMPROVEMENTS.md` (read-only usability review).
That note lists interaction gaps; this plan is about *visual density and
hierarchy* — the sidebar currently looks like a control panel, not a session
list. Several items overlap and are reconciled in §5.

## 1. Goal

One screen, one job: **pick a session or start one**. Everything else is
secondary and earns its space.

Target, at the default 900 px window height:

| | Today | After |
| :--- | ---: | ---: |
| Fixed chrome above/below the list | ≈521 px | ≈202 px |
| Session rows visible | ≈4–5 | ≈12 |

Non-goals (do not touch in this pass):

- Palette, type scale, radii, `size::*` tokens, `DISPLAY_FONT`/`MONO_FONT`.
- Grouping semantics, filter semantics, `SESSION_PAGE`/`ARCHIVE_PAGE` paging,
  search matching, subagent hierarchy, drag-to-dock, session persistence.
- The compact drawer's placement, the auto-collapse rules in `layout.rs:543`,
  or `MIN_WIDTH = 264.` (`sidebar.rs:35`, enforced by `docking.rs:263`).

## 2. What the sidebar renders today

Order is exactly the child order at `sidebar.rs:458–622`.

| Region | Source | Height |
| :--- | :--- | ---: |
| Brand row: grip, `brand::mark(30)`, "Hooter" at 21 px, close button | `sidebar.rs:111–155` | 72 |
| `New agent` (accent-soft, ⌘N) | `sidebar.rs:157–170` | 44 + 6 |
| `Open folder` (⌘O) | `sidebar.rs:171–179` | 38 |
| `Terminal` (⌃ plus the grave key) | `sidebar.rs:181–186` | 38 |
| Search `Input` with magnifier prefix | `sidebar.rs:188–198` | ≈58 incl. margins |
| Filter chips, 2 rows × 2 (`filter_chip`, `sidebar.rs:665–712`) | `sidebar.rs:341–417` | 68 |
| Grouped session list (scroll) | `sidebar.rs:464–511` | flex |
| Archived: header + capped list | `sidebar.rs:512–621` | ≈142 |
| Footer: Settings row + 98 px grouping cycler | `sidebar.rs:418–441` | 55 |

A session row (`session_row`, `sidebar.rs:773–1119`) is three text lines plus a
24 px action slot: `py(12)`, title at `size::BASE`, subtitle + clock at
`size::XS`, then an optional branch/token line — ≈78–86 px.

## 3. Diagnosis

1. **The sidebar repeats the app's identity.** The window titlebar already says
   the product name, and the welcome screen has the mark. Inside the panel the
   72 px brand row is the single largest fixed block and the smallest
   information per pixel.
2. **Three nav rows for one primary action.** `New agent` is the job;
   `Open folder` and `Terminal` are occasional and already have ⌘O and the
   ⌃ grave key plus palette entries. They cost 76 px of permanent chrome for
   shortcuts nobody needs to see daily.
3. **The filter strip reads as a form.** Two rows of two with a reserved 30 px
   count slot (`sidebar.rs:697–711`). Four values do not need two rows.
4. **Rows are over-specified.** Three lines per row for title, status, branch and
   token total; at 264 px most of the third line truncates anyway
   (`details_line`, `sidebar.rs:766`). Tokens are a diagnostic, not a
   navigation aid.
5. **Archived is always open.** Up to 142 px (`sidebar.rs:512`) for something
   opened rarely, with no disclosure control.
6. **Grouping is a cycling button.** `sidebar.rs:372–404` truncates its label in
   98 px and only explains itself in a tooltip; the count slots and ⌘⇧G are
   invisible.
7. **Every row paints its ellipsis at full opacity** (`sidebar.rs:1011–1038`),
   competing with the status glyph that carries the meaning.

## 4. Target layout

| Region | Spec | Height |
| :--- | :--- | ---: |
| Header | grip · "Sessions" (`size::SM`, medium, `p.text_muted`) · live `needs you / working` indicator · close button | 40 |
| Primary action | `+ New agent` flex-1 accent-soft; `Open folder` and `Terminal` as 28 px icon buttons with tooltips | 36 + 8 |
| Search | `Input::small()` flex-1, `⌘F` hint in the placeholder | 32 + 8 |
| Filters | one row of four segmented chips: icon + count, equal flex widths | 28 + 8 |
| Session list | unchanged mechanics, two-line rows | flex |
| Archived | 30 px disclosure row, collapsed by default | 30 |
| Footer | Settings row (keeps `>= 140 px` for the dock fixture) + grouping **menu** button | 44 |

Rules:

- Palette only: `p.sidebar_fill`, `p.border`, `p.raised`, `p.accent_soft`,
  `p.text/_muted/_subtle`, `p.warning`. No new colors, no new tokens.
- One accent surface in the panel: the `New agent` row. Active session rows keep
  their `p.accent_soft` fill; nothing else may use it.
- 8 px vertical rhythm; horizontal gutter `px(10)` for controls, `px(20)` for
  group headings, `px(12)` inner row padding (as today).
- Type: `size::SM` for the panel title and group headings, `size::XS` for every
  secondary line, `size::BASE` for session titles. No new sizes.
- Controls stay ≥ 28 px tall, rows ≥ 56 px, so pointer targets do not regress.
- Everything interactive keeps `role(Button)`, `aria_label`, `tab_index` and a
  `focus_visible` border.

## 5. Work packages

Each package is independently reviewable and revertable. Land them in order;
P1 and P2 alone deliver most of the visual change.

### P0 — Baseline evidence

Capture before screenshots and numbers so the change is provable, not asserted:
default window, 1000×560, 0/4/40 sessions, archives present, one group vs five
groups. Record rows visible per state. Reuse the existing offline sweep harness
under `tools/blueprint/`; write outputs outside the repo. Add the before/after
table to `docs/UI-AUDIT.md` when the work lands (that file is the evidence log
for UI passes; `STATE.md:89` already tracks a plan the same way).

### P1 — Header and primary action (`sidebar.rs:111–186`)

- Replace the brand row with a 40 px header: `docking::handle(Panel::Sidebar)`,
  `ui::label("Sessions", size::SM, p.text_muted)` id `sidebar-heading`, the
  existing needs-you/working indicator (reuse the `group-waiting`/
  `group-working` glyph treatment, ids `sidebar-waiting` / `sidebar-working`),
  and the existing close button.
- Keep `hide-sidebar` and its click handler verbatim (`sidebar.rs:130–152`):
  `collapsed_sidebar_reopens_docked_below_the_window_buttons` and
  `compact_sessions_drawer_stays_below_the_window_buttons` depend on it.
- Drop `crate::brand::mark` and the `hooter-brand` label from the panel. Nothing
  in `crates/flint-app/tests` finds `hooter-brand`; the mark stays in the
  titlebar, welcome screen and `assets/`.
- `New agent` becomes `h(36)`, `mb(8)`, same accent-soft fill and ⌘N hint.
  `Open folder` and `Terminal` become 28×28 icon buttons in the same row,
  right-aligned, keeping ids `open-folder` and `terminal` (fixtures
  `tests/blueprint_ui.rs:6670/6723/6742` click `terminal`).
- Use `nav_row` only for the footer afterwards; delete its `new-agent`
  special-casing (`sidebar.rs:737–750`) if it becomes dead.

Acceptance: panel shows one title, one primary action; chrome above the search
drops from ≈248 px to ≈84 px; `sidebar`, `hide-sidebar`, `new-agent`,
`open-folder`, `terminal` all still resolve by id.

### P2 — Search and filters (`sidebar.rs:188–198`, `341–417`, `665–712`)

- Search: `Input::new(&app.search).id("session-search").small()` — the id and
  `cleanable(true)` stay; only height changes.
- Filters: one row (`h(28)`), four equal chips, each `icon + count`:
  `All` keeps its word (it has no natural glyph), the three status filters use
  the same glyphs as session rows — `ShieldAlert` (Needs you), `Loader`
  (Working), `Check` (Ready). `aria_label` keeps the full name and count
  (`format!("{label}, {count}")`), so screen readers lose nothing.
- Keep ids `("session-filter", ix)` for `0..4` in the same order
  (All, NeedsYou, Working, Ready) and keep `flex_1` + `min_w_0`. Two fixtures
  compare those bounds before/after live counts change
  (`sidebar_filter_positions_survive_multi_digit_counts`,
  `sidebar_filter_chips_stay_in_place_when_counts_change`); equal widths in one
  row keep both passing without touching them.
- Zero counts render as `0` in `p.text_subtle` rather than being dropped — the
  slot must not move. Selected chip: `p.raised` fill, `p.text` label.
- Keep the empty-list recovery block (`sidebar.rs:478–507`) as is.

Acceptance: strip height 68 → 36 px; four chips stay in one row at 264 px and
at the docked minimum; both count-stability fixtures pass unchanged.

### P3 — Two-line session rows (`sidebar.rs:773–1119`)

- `py(12)` → `py(8)`; row height ≈78–86 → ≈58.
- Line 1: status glyph (unchanged 16 px column) · title (`size::BASE`) ·
  `sidebar-when` clock right-aligned (unchanged id and position).
- Line 2 becomes a flex row, all segments `min_w_0` + `truncate`:
  `[⎇ branch]` · `[status text]` · `[clock]` — clock is already right-aligned
  on this line today, so the row reads: branch, status, time.
  - The branch segment keeps id `sidebar-branch`, renders only when
    `project::branch` is `Some`, and truncates before the status does
    (`flex_shrink(1)` on both, branch first).
  - `sidebar-subtitle` stays on the status segment (id used by
    `tests/blueprint_ui/failure_status.rs:31/75/150` and
    `tests/blueprint_ui.rs:3203`).
- Retire the third line: delete the `sidebar-details` / `sidebar-usage`
  elements and the `details_line` helper. Token totals move into the row's
  title tooltip (`format!("{title}\n{branch}\n{tokens} tokens")`), which already
  exists at `sidebar.rs:886`.
- Ellipsis (`session-actions`): keep the 24 px slot and the id, but paint the
  glyph only on hover, focus or when the row is active — `opacity(0.)` by
  default, `1.` in those states, so bounds stay fixed while the noise goes.
- Keep `drag_row` wrapping and the inline rename path untouched.

Acceptance: three-line rows are gone; no row exceeds 60 px; title and clock
bounds are unchanged in `sidebar_timestamp_does_not_take_space_from_the_title`;
the branch still truncates inside the panel.

### P4 — Group headings and the archive disclosure

- Headings (`sidebar.rs:240–306`): `h(24)`, `size::XS`, 11 px chevron
  (`ChevronDown`/`ChevronRight`), `role(Button)`, `tab_index(0)`, click toggles.
  Keep ids `("workspace-label", g)`, `("group-waiting", g)`,
  `("group-working", g)`; the chevron gets `("group-toggle", g)`.
  Counts (`working`, `waiting`, `n of N`) render only when non-zero — same
  rules as today, just quieter and on one 24 px line.
- Collapsed groups render heading only. State lives in
  `FlintApp.collapsed_groups: HashSet<GroupKey>` and persists as
  `Settings.collapsed_groups: Vec<String>` (`settings.rs:154` is the precedent
  for `session_grouping`). Add `GroupKey::setting_key() -> String`
  (`project:<root>|<subdir>`, `status:<bucket>`, `agent:<kind>`) and
  `GroupKey::from_setting_key`. Unknown keys are dropped on load; a group that
  has no sessions is pruned on save. `#[serde(default)]` keeps old settings
  files loading.
- Archived (`sidebar.rs:512–621`): wrap in a 30 px disclosure row
  id `archived-sessions`, `role(Button)`, aria `"Archived, N sessions"`, chevron
  matching the group headings. Collapsed by default, remembered as
  `Settings.archived_open: bool` (default `false`). Expanded keeps the current
  cap (112 px, 76 px in the compact drawer) and `show-more-archives`.
- Collapsed-by-default means the archive fixture
  (`sidebar_archive_scrollbar_keeps_restore_and_navigation_reachable`,
  `tests/blueprint_ui.rs:3072`) must click `archived-sessions` before reading
  `archive-list`; update that fixture with a comment saying why.

Acceptance: with archives present and nothing collapsed by hand, the list gains
≈112 px; group headings toggle with mouse and keyboard; collapsed state
survives a relaunch (mirror `sidebar_grouping_is_remembered_across_launches`).

### P5 — Footer: a real grouping control (`sidebar.rs:418–441`)

- Keep the `settings` row (`nav_row`, id `settings`, `>= 140 px` wide) — the
  dock-resize fixture asserts that width.
- Replace the 98 px cycler with a menu button id `session-grouping`:
  `Layers` icon + current label + `ChevronDown`, `w(98)`, `h(24)`. Click opens
  an inline panel below the button (the pattern already used for the row menu
  at `sidebar.rs:1067–1107`), listing Project / Status / Agent with a check
  glyph on the current one, ids `("grouping-option", 0..3)`, plus the ⌘⇧G hint.
  New state `FlintApp.sidebar_menu: Option<SidebarMenu>` with the variant
  `SidebarMenu::Grouping`; Escape closes it in the `app_input.rs`/`layout.rs`
  chain next to `session_menu.take()` (`layout.rs:184`).
- `CycleSessionGrouping` (⌘⇧G) keeps working and should also close the menu.
- `sidebar_groups_by_project_status_or_agent_and_counts_hidden_sessions`
  (`tests/blueprint_ui.rs:3329/3337`) clicks `session-grouping` twice and expects
  Project→Status→Agent. Update it to open the menu and click
  `("grouping-option", 1)` / `("grouping-option", 2)`, and keep the ⌘⇧G step at
  the end as the keyboard path. `sidebar_dock_resizing_keeps_controls_readable`
  only reads bounds and stays green.

Acceptance: grouping is visible without a tooltip; menu closes on Escape,
outside click and selection; ⌘⇧G still cycles.

### P6 — Interaction floor (only what the density change makes worse)

Not a redesign — three fixes so a shorter list stays navigable:

1. **Roving focus** in the list: one tab stop for `session-list`, Up/Down/Home/
   End move between visible rows, Enter opens, Right/Shift+F10 opens the row
   menu, reusing `reveal_row` (`sidebar.rs:79`). Rows drop `tab_index(0)` in
   favour of `tab_index(-1)` plus an explicit roving index. This is item 1 of
   `docs/SIDEBAR-IMPROVEMENTS.md`; it must land with this pass because
   `session-actions` no longer shows a glyph by default, so tab-to-menu
   discovery changes.
2. **⌘F focuses the search**; Escape clears it when non-empty, inserted into the
   escape chain before the drawer step (`layout.rs:210`). Add the binding to
   `lib.rs:bind_keys` and the label to `shortcuts.rs` consumers and the README
   key table (`README.md:332`).
3. **Auto-extend the page** when `sidebar_scroll` reaches `max_offset()`, keeping
   `show-more-sessions` (`sidebar.rs:311–324`) for discoverability.

Defer to a later pass: collapse-all, "back to current session" row, hover
previews, archived search. Note them in `docs/SIDEBAR-IMPROVEMENTS.md` as
outstanding.

## 6. Test contract

`crates/flint-app/tests/blueprint_ui.rs` (7123 lines) plus
`tests/blueprint_ui/sidebar.rs` hold ~40 sidebar fixtures, most of them
bounds-based. Treat element ids as the contract.

Must keep passing **unchanged** (ids and bounds semantics preserved):

- `sidebar_dock_resizing_keeps_controls_readable` (`sidebar` ≥ 264 px,
  `settings` ≥ 140 px, `settings.right() <= grouping.left()`,
  `grouping.right() <= sidebar.right()`).
- `sidebar_filter_positions_survive_multi_digit_counts`,
  `sidebar_filter_chips_stay_in_place_when_counts_change`.
- `sidebar_timestamp_does_not_take_space_from_the_title`,
  `sidebar_fresh_empty_timestamp_does_not_reserve_title_space`.
- `sidebar_long_workspace_titles_and_statuses_stay_inside_the_panel`.
- `sidebar_search_and_empty_filters_recover_and_reset_scroll`,
  `sidebar_search_matches_the_last_message`, `sidebar_needs_you_filter_...`.
- `sidebar_lists_a_page_of_sessions_then_shows_more`,
  `sidebar_paging_always_keeps_the_active_session_listed`.
- `sidebar_session_actions_work_with_mouse_and_keyboard_without_selecting`,
  `sidebar_rename_*`, `sidebar_escape_dismisses_actions_and_rename_...`.
- `collapsed_sidebar_reopens_*`, `compact_sessions_drawer_stays_below_...`,
  `the_sidebar_opens_the_terminal_dock`,
  `sidebar_grouping_is_remembered_across_launches`,
  `sidebar_rows_hold_still_while_streaming_and_track_progress`.
- All `tests/blueprint_ui/failure_status.rs` and `subagents.rs` sidebar fixtures.

Must be **updated**, each with a one-line comment naming this plan:

| Fixture | Why |
| :--- | :--- |
| `sidebar_groups_by_project_status_or_agent_and_counts_hidden_sessions` | grouping is now a menu, not a cycler |
| `sidebar_archive_scrollbar_keeps_restore_and_navigation_reachable` | archives start collapsed |
| `sidebar_long_branch_preserves_readable_metadata` (`tests/blueprint_ui/sidebar.rs:64`) | `sidebar-details`/`sidebar-usage` retired; assert the branch truncates and tokens move to the title tooltip |
| any fixture asserting a three-line row height | rows are 58 px |

New fixtures, one per acceptance above:

- `sidebar_primary_actions_fit_one_row` — `new-agent`, `open-folder`,
  `terminal` on one line at 264 px, all inside the panel.
- `sidebar_chrome_leaves_most_of_the_panel_to_sessions` — with 20 sessions,
  `session-list` height ≥ 60 % of the panel at 900×900 and ≥ 3 rows at 1000×560.
- `sidebar_filters_stay_in_one_row_at_minimum_width`.
- `sidebar_branch_and_status_share_one_line_without_clipping_the_clock`.
- `sidebar_row_actions_appear_on_hover_and_focus` — glyph opacity 0 → 1, bounds
  unchanged.
- `sidebar_groups_collapse_and_remember_it` — toggle, relaunch, still collapsed.
- `sidebar_archived_disclosure_remembers_its_state`.
- `sidebar_grouping_menu_offers_all_three_orders`.

## 7. Verification

Run in this order, from the repo root, on a machine with a display (the
`blueprint_ui` suite needs one):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p flint-app --lib
cargo test -p flint-app --test blueprint_ui -- --test-threads=1
cargo test --workspace
cargo build --release -p flint-app
```

`FLINT_LIVE=0`, disposable homes and workspaces, no provider credentials — the
same conditions recorded in `GATES.md`. The serial `--test-threads=1` run is
required: the suite drives real windows.

Report, per package, the measured before/after for: chrome height above the
list, rows visible at 900×900 and 1000×560, and the two count-stability
fixtures. Screenshots go under a disposable root and are referenced from
`docs/UI-AUDIT.md`; if images cannot be inspected, say so rather than claiming
visual approval (the precedent is `GATES.md`, which records "not visually
approved").

## 8. Risks

| Risk | Mitigation |
| :--- | :--- |
| Density change breaks a bounds fixture in a way that looks like a real regression | Land P1–P3 behind one reviewable diff; the table in §6 is the checklist. Fix fixtures only where the *design* changed, never to silence a real overflow. |
| Losing the branch from the row hurts worktree work | The branch stays, on line 2, first and truncating; only the token count moves to the tooltip. |
| Two-line rows at 264 px clip the clock | `sidebar-when` stays `flex_shrink_0`; the branch and status truncate before it. Covered by `sidebar_branch_and_status_share_one_line_without_clipping_the_clock`. |
| Collapsed archives hide a restore path | The disclosure row always shows `Archived · N` and is one click; `restore_archive` is unchanged. |
| Persisting collapsed groups grows `Settings` | One `Vec<String>` plus one `bool`, `#[serde(default)]`, pruned on save. |
| Icon-only filters are less obvious | Same glyphs as the rows, full `aria_label`, tooltip on each chip, and the empty-state block still offers "Show all sessions". |
| Roving focus conflicts with the drawer focus trap | Reuse `ScrollableRow`'s focus handles; keep `active_focus_trap` assertions in `compact_sessions_drawer_stays_below_the_window_buttons` green. |

Rollback: each package is a self-contained diff. P1–P5 touch only `sidebar.rs`
plus `settings.rs` (P4) and `app.rs` (P4, P5 state); reverting any one restores
the previous geometry without touching persistence beyond dropping an unused
field.

## 9. Open questions

1. Should the header's `needs you / working` indicator be clickable (filter to
   Needs you) or purely informational? Recommendation: clickable in P6, since
   the chips are now icon-only.
2. Is 12 rows at 900 px enough, or should the panel also remember a user-set
   width so wide-window users get two columns of metadata? Out of scope here;
   `docking.rs` already resizes the panel, so this is a follow-up.
3. Should `size::SM`'s doc comment ("Mono text: code, paths, commands") be fixed
   while it is used for UI labels? Unrelated; leave it, or fix separately.
