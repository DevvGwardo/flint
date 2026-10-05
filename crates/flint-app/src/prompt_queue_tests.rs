use super::*;
use pretty_assertions::assert_eq;

#[test]
fn queue_reorders_edits_and_preserves_attachment_snapshots() {
    let mut queue = Queue::default();
    let first = queue
        .enqueue(Prompt::new(
            "first".into(),
            "first\n\n[Attached file: captured]".into(),
            Vec::new(),
        ))
        .unwrap();
    let second = queue
        .enqueue(Prompt::new(
            "second".into(),
            "second".into(),
            vec![ImageAttachment {
                name: "fixture.png".into(),
                mime_type: "image/png".into(),
                data: "fixture".into(),
            }],
        ))
        .unwrap();
    queue.move_by(second, -1);
    assert_eq!(
        queue
            .items
            .iter()
            .map(|prompt| prompt.id)
            .collect::<Vec<_>>(),
        [second, first]
    );
    let changed = queue.items[1].with_text("edited".into());
    assert_eq!(changed.message, "edited\n\n[Attached file: captured]");
    queue.replace(first, changed).unwrap();
    assert_eq!(queue.remove(first).unwrap().text, "edited");
    assert_eq!(queue.items[0].images[0].data, "fixture");
}

#[test]
fn queue_limits_preserve_existing_items() {
    let mut queue = Queue::default();
    for _ in 0..MAX_PROMPTS {
        queue
            .enqueue(Prompt::new("fixture".into(), "fixture".into(), Vec::new()))
            .unwrap();
    }
    assert!(
        queue
            .enqueue(Prompt::new("extra".into(), "extra".into(), Vec::new()))
            .is_err()
    );
    assert_eq!(queue.items.len(), MAX_PROMPTS);
    queue.items.clear();
    let huge = Prompt::new(String::new(), "x".repeat(MAX_BYTES + 1), Vec::new());
    assert!(queue.enqueue(huge).is_err());
    assert!(queue.items.is_empty());
}

#[test]
fn saved_queues_restore_paused_without_losing_images_or_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut queue = Queue::default();
    queue
        .enqueue(Prompt::new(
            "fixture".into(),
            "fixture".into(),
            vec![ImageAttachment {
                name: "fixture.png".into(),
                mime_type: "image/png".into(),
                data: "image snapshot".into(),
            }],
        ))
        .unwrap();
    queue
        .enqueue(Prompt::new("next".into(), "next".into(), Vec::new()))
        .unwrap();
    queue.save(dir.path()).unwrap();
    let restored = Queue::load(dir.path()).unwrap();
    assert!(restored.paused);
    assert_eq!(restored.items, queue.items);
    assert_eq!(restored.steer_after_turn, None);
}

#[test]
fn corrupt_and_failed_queue_files_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prompt-queue.json");
    std::fs::write(&path, "invalid fixture").unwrap();
    assert!(Queue::load(dir.path()).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "invalid fixture");
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(Queue::default().save(dir.path()).is_err());
    assert!(path.is_dir());
}

#[test]
fn editing_keeps_non_prefix_context_and_images_bounded_on_load() {
    let prompt = Prompt::new(
        "Run command".into(),
        "original command output".into(),
        Vec::new(),
    );
    assert!(
        prompt
            .with_text("Review output".into())
            .message
            .contains("original command output")
    );
    let dir = tempfile::tempdir().unwrap();
    let mut queue = Queue::default();
    let images = (0..=crate::image_attach::MAX_IMAGES)
        .map(|_| ImageAttachment {
            name: "fixture.png".into(),
            mime_type: "image/png".into(),
            data: "fixture".into(),
        })
        .collect();
    queue
        .enqueue(Prompt::new("fixture".into(), "fixture".into(), images))
        .unwrap();
    queue.save(dir.path()).unwrap();
    assert!(Queue::load(dir.path()).is_err());
}

#[test]
fn escaped_payloads_cannot_replace_a_valid_queue_with_an_unloadable_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut queue = Queue::default();
    queue
        .enqueue(Prompt::new(
            "retained".into(),
            "retained".into(),
            Vec::new(),
        ))
        .unwrap();
    queue.save(dir.path()).unwrap();
    let path = dir.path().join("prompt-queue.json");
    let original = std::fs::read(&path).unwrap();
    let escaped = "\0".repeat(3 * 1024 * 1024);
    queue
        .enqueue(Prompt::new(escaped.clone(), escaped, Vec::new()))
        .unwrap();
    assert!(queue.save(dir.path()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(Queue::load(dir.path()).unwrap().items.len(), 1);
}
