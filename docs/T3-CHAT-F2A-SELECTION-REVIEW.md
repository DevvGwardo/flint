# F2a selection-policy finding and native compatibility proposal

Status: F2a remains incomplete; same frontend owner may propose a narrowly
scoped native compatibility slice. No toolkit/root-manifest/lock edit yet.
Frozen selection requirements are unchanged, not weakened after failure.

## Actual evidence and inspected APIs

Worker's final F2a actual-view run: four passed, one failed. Selection survives
opening settings, then is cleared while clicking a width choice before save/
reflow. Full regressions, formatting and strict app Clippy are still pending.
Raw report is run-root `specs/frontend-f2a-results.md`; retain all failures.

Installed GPUI Base 0.7.0 `text_selection.rs:2659–2708` clears during Capture
before checking suppression in Bubble. `prepare_for_mouse_down:1480–1499`
clears logical endpoints and participant-local selection. Width invalidation
cannot restore an already-cleared selection.

Primary inspection additionally established:

- Touch UI bounds do not solve pointer selection: `TouchSelection::covers`
  requires `touch.active` at lines 1071–1079. Do not spoof a touch/long press.
- Public TextSelection/TextViewState exposes snapshots, clear/select-all,
  touch operations and reflow invalidation, not exact pointer-range restoration.
- GPUI Pre `window.rs:5760–5778` stops both later Capture and all Bubble
  listeners after capture cancellation. Do not manually redispatch/simulate
  clicks, replace native button handling or globally suppress ordinary selection.
- Base Root paints TextSelectionLayer before application content at
  `root.rs:280–308`. A generic plugin wrapper is not an exact range-restore API.

This is a concrete native-policy gap. It is not permission to edit a shared
installed Cargo cache or silently change initialization/security policy.

## Proposed separately coordinated native slice

The same frontend owner may write only
`specs/frontend-f2a-native-phase0.md` with real APIs, numbered disagreements,
exact ownership, offline dependency/provenance/lock plan and regression gates.
No implementation or Cargo/native job in this proposal.

Propose the smallest opt-in native preserving-control API that is consulted
before pointer Capture clears selection. It applies only to consciously marked
Copy/Stage/width/settings controls, using actual visible/occlusion-aware native
hitboxes in the current window/frame. Default clicks, text drag/shift/double
click, normal input/selection, touch menu/handles, other controls/windows and
inactive pane routing keep existing behavior. No global no-clear mode, fake
pointer/touch event, private field mutation or post-hoc select-all substitute.

Bound per-frame registrations, reject stale/removed/occluded control geometry,
end transient selection dragging without losing the logical range, and keep
keyboard/assistive native button activation and disabled semantics intact.
App controls retain their existing ownership/revision guards and cannot mint
engine/task/approval authority.

Evaluate a repository-owned copy of the **locked** GPUI Base 0.7.0 under a
new crate path (not the installed registry), with a narrow crates.io path patch.
The cached source is 208 regular files/4,125,701 bytes; this is measured
footprint, not an approval to copy it. Verify provenance against the cached
crate archive and existing lock checksum; preserve Apache license/source
attribution and document modified files. No acquisition or version upgrade.

The proposal must explicitly coordinate any new workspace member/exclusion,
root manifest and only necessary lock source/checksum reconciliation. Original
root/cache bytes and user work remain preserved. Choose a location already
covered by current source fingerprints or propose the exact tooling addition
before relying on it. No absolute machine-local dependency in shipped source.

## Required unchanged and extended evidence

Keep the original failing width-choice test and add selected **transcript**
Copy/Stage activation (the earlier passing test covers composer selection only),
physical resize, retained entity/source range/tail/focus and narrow tiled/child
coverage. Test ordinary outside/occluded/removed controls still clear/route as
before, no cross-window exemption and unchanged pointer/touch/keyboard behavior.

Complete stale append/replacement, nested/incomplete fence actions and isolated
Stage no-filesystem-effect evidence. Native compatibility tests, original F1
and F2a actual-view tests, relevant app regressions, owned-file formatting and
strict affected checks must pass before acceptance. Legacy/native/provider/
approval/resource semantics and original binaries remain unchanged.

Implementation needs a separate frozen native-slice ruling after this
proposal. No F2b highlighting or F3, duplicate frontend/native worker, renderer
permission, dependency download, user-app restart, commit or publishing.
