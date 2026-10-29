//! Fail-closed queue directory handling: missing directories never panic,
//! corrupt saves pause instead of losing history.

#[test]
fn queue_dir_resolution_assigns_and_returns_a_dir() {
    let home = tempfile::tempdir().unwrap();
    let mut dir: Option<std::path::PathBuf> = None;
    let resolved = flint_app::queue_actions::resolve_queue_dir(home.path(), &mut dir);
    assert!(resolved.is_some());
    assert_eq!(dir, resolved);
}

#[test]
fn queue_dir_resolution_keeps_an_existing_dir() {
    let home = tempfile::tempdir().unwrap();
    let existing = home.path().join("sessions").join("kept");
    let mut dir = Some(existing.clone());
    assert_eq!(
        flint_app::queue_actions::resolve_queue_dir(home.path(), &mut dir),
        Some(existing)
    );
}

#[test]
fn restore_queue_with_missing_dir_is_paused_not_a_panic() {
    let queue = flint_app::app_store::load_queue_for_restore(None);
    assert!(queue.paused);
    assert!(queue.unreadable);
    assert_eq!(
        queue.error.as_deref(),
        Some("Session directory is missing.")
    );
}

#[test]
fn restore_queue_with_missing_file_loads_empty() {
    let home = tempfile::tempdir().unwrap();
    let queue = flint_app::app_store::load_queue_for_restore(Some(home.path()));
    assert!(!queue.paused);
    assert!(!queue.unreadable);
    assert!(queue.items.is_empty());
}

#[test]
fn restore_queue_with_corrupt_file_is_paused_and_kept() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("prompt-queue.json"), b"{not json").unwrap();
    let queue = flint_app::app_store::load_queue_for_restore(Some(home.path()));
    assert!(queue.paused);
    assert!(queue.unreadable);
    assert!(queue.error.is_some());
}
