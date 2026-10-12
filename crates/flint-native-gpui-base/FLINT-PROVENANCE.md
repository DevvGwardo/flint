# Flint locked native source provenance

Package: `gpui-base` 0.7.0. License: Apache-2.0; retain `LICENSE-APACHE` and all packaged attribution.

N0 source is an unmodified copy of the existing locked crates.io archive, not a downloaded/version-upgraded fork.

- Archive filename: `gpui-base-0.7.0.crate`
- Archive/root-lock original SHA-256: `64bb52b29a8dcdc8d3f595b8b0839c076f237572ab601ba4577d3da8ae085728`
- Original regular files: 207; original bytes: 4125694.
- All original relative-file bytes, modes, manifests, nested locks, resources and build scripts verified against the archive before adding this document.
- `.cargo-ok` was not an archive member and was not copied. No link/special/duplicate/unsafe archive member was accepted.
- Use the normalized `Cargo.toml`. `Cargo.toml.orig` and the packaged `Cargo.lock`, where present, are preserved provenance, not replacement Flint workspace/lock inputs.
- Original source changes in N0: **none**. The only added file is this document. No selection API, opacity accessor or app action implementation is activated by this copy.
- The Flint root uses a relative crates.io path patch and explicitly excludes this package from workspace membership.
- Full upstream/final inventories and source brackets: run-root `evidence/frontend-f2a-native-n0-before/archives-original-inventory.json` and `evidence/frontend-f2a-native-n0-final-inventory.json`.

This copy was separately authorized by frozen `docs/T3-CHAT-F2A-NATIVE-RULINGS.md`. Later source changes require N1 authorization and exact changed-file attribution; no implicit permission, dependency acquisition or native GUI execution follows.

## 2026-10-06 Settings-modal follow-up

`src/text_selection.rs` now has a window-local mouse-suspension flag,
`TextSelection::suspend_mouse`, and a mouse-down capture guard. Opening Settings
ends the current drag but retains the selected text; closing Settings resumes
normal pointer selection. Input fields keep their own selection behavior.
The app's real-view regressions cover width changes plus Save, Cancel and Escape.
This does not certify or close the historical F2a/N1 gates above.

Standalone native tests remain blocked by the locked offline dev graph:
`gpui-pre-reqwest-client` is not cached. Root package selection also refuses
testing this excluded dependency with dev-dependencies. No package download,
native manifest/lock regeneration, or workspace-membership change was made.
