use super::*;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

fn reference(records: &[Logged]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, record).unwrap();
        bytes.push(b'\n');
    }
    bytes
}

fn records(count: usize) -> Vec<Logged> {
    (0..count)
        .map(|n| Logged::User(format!("{n}: 界🙂\n\"quoted\"\\tail")))
        .collect()
}

fn replay_saved_records(dir: &Path) -> Vec<Logged> {
    write_meta(
        dir,
        &Meta {
            id: "fixture".into(),
            title: None,
            workspace: dir.into(),
            subagent_workspaces: Vec::new(),
            created_at: 0,
            updated_at: 0,
            agent: AgentKind::Flint,
            general: false,
        },
    )
    .unwrap();
    load(dir).unwrap().1
}

#[test]
fn append_batch_matches_jsonl_oracle_for_large_and_nested_records() {
    let root = tempfile::tempdir().unwrap();
    let mut records = records(4_000);
    records.push(Logged::User("x".repeat(8 * 1024 * 1024)));
    records.push(Logged::Event(AgentEvent::SubagentEvent {
        call_id: "parent".into(),
        event: Box::new(AgentEvent::TextDelta("child\n界🙂".into())),
    }));
    append_batch(root.path(), &records).unwrap();
    assert_eq!(
        std::fs::read(root.path().join("events.jsonl")).unwrap(),
        reference(&records)
    );
    let replay: Vec<Logged> = std::fs::read_to_string(root.path().join("events.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(reference(&replay), reference(&records));
}

#[test]
fn empty_append_batch_does_not_create_storage() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("unused");
    append_batch(&dir, &[]).unwrap();
    assert!(!dir.exists());
}

#[test]
fn interrupted_unterminated_event_tail_cannot_swallow_a_new_record() {
    let root = tempfile::tempdir().unwrap();
    let prefix = reference(&[Logged::User("kept prefix".into())]);
    let damaged = br#"{"Event":{"TextDelta":"interrupted"#;
    let path = root.path().join("events.jsonl");
    std::fs::write(&path, [&prefix[..], damaged].concat()).unwrap();
    append(root.path(), &Logged::User("new prompt".into())).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(&prefix));
    let replay = replay_saved_records(root.path());
    assert_eq!(
        reference(&replay),
        reference(&[
            Logged::User("kept prefix".into()),
            Logged::User("new prompt".into()),
        ])
    );
    assert!(
        bytes.windows(damaged.len()).any(|window| window == damaged),
        "repair must preserve the interrupted bytes"
    );
}

#[test]
fn a_complete_last_record_without_a_newline_survives_the_next_append() {
    let root = tempfile::tempdir().unwrap();
    let first = Logged::User("last complete prompt".into());
    std::fs::write(
        root.path().join("events.jsonl"),
        serde_json::to_vec(&first).unwrap(),
    )
    .unwrap();
    append(root.path(), &Logged::User("next prompt".into())).unwrap();
    let replay = replay_saved_records(root.path());
    assert_eq!(
        reference(&replay),
        reference(&[first, Logged::User("next prompt".into())])
    );
}

#[test]
fn interrupted_unicode_tail_preserves_valid_records_through_production_load() {
    let root = tempfile::tempdir().unwrap();
    let first = Logged::User("kept 界🙂 prefix".into());
    let prefix = reference(std::slice::from_ref(&first));
    let damaged = [br#"{"User":"interrupted "#.as_slice(), &[0xf0, 0x9f, 0x99]].concat();
    let original = [&prefix[..], &damaged].concat();
    assert!(std::str::from_utf8(&original).is_err());
    let path = root.path().join("events.jsonl");
    std::fs::write(&path, &original).unwrap();
    let next = Logged::User("new 界🙂 prompt".into());
    append(root.path(), &next).unwrap();
    let kept = std::fs::read(&path).unwrap();
    assert!(
        kept.starts_with(&original),
        "damaged bytes must remain on disk"
    );
    let replay = replay_saved_records(root.path());
    assert_eq!(reference(&replay), reference(&[first, next]));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        kept,
        "loading must not rewrite history"
    );
}

#[derive(Default)]
struct Captured(Vec<u8>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn buffered_serialization_matches_reference_across_buffer_boundaries() {
    for size in [
        0,
        EVENT_WRITE_BUFFER_BYTES - 16,
        EVENT_WRITE_BUFFER_BYTES,
        EVENT_WRITE_BUFFER_BYTES + 1,
        8 * 1024 * 1024,
    ] {
        let mut records = records(16);
        records.push(Logged::User(format!("{}\n界🙂\"\\", "x".repeat(size))));
        let mut captured = Captured::default();
        write_records(&mut captured, &records).unwrap();
        assert_eq!(captured.0, reference(&records));
    }
}

struct FailedFile<'a> {
    file: &'a mut std::fs::File,
    remaining: usize,
    fail_flush: bool,
}

impl std::io::Write for FailedFile<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Err(std::io::Error::other("injected partial write"));
        }
        let count = self.file.write(&bytes[..bytes.len().min(self.remaining)])?;
        self.remaining -= count;
        Ok(count)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.fail_flush {
            Err(std::io::Error::other("injected flush failure"))
        } else {
            self.file.flush()
        }
    }
}

fn failed_append_rolls_back(remaining: usize, fail_flush: bool, large: bool) {
    let root = tempfile::tempdir().unwrap();
    append(root.path(), &Logged::User("existing prefix".into())).unwrap();
    let path = root.path().join("events.jsonl");
    let prefix = std::fs::read(&path).unwrap();
    let records = if large {
        vec![Logged::User("x".repeat(8 * 1024 * 1024))]
    } else {
        records(4_000)
    };
    let err = append_batch_with(root.path(), &records, |file, batch| {
        write_records(
            &mut FailedFile {
                file,
                remaining,
                fail_flush,
            },
            batch,
        )
    })
    .unwrap_err();
    assert!(err.to_string().contains("injected"));
    assert_eq!(std::fs::read(&path).unwrap(), prefix);
    append_batch(root.path(), &records).unwrap();
    let expected = [prefix, reference(&records)].concat();
    assert_eq!(std::fs::read(&path).unwrap(), expected);
}

#[test]
fn partial_buffer_write_rolls_back_before_retry() {
    failed_append_rolls_back(1_024, false, false);
}

#[test]
fn partial_oversized_record_rolls_back_before_retry() {
    failed_append_rolls_back(EVENT_WRITE_BUFFER_BYTES + 37, false, true);
}

#[test]
fn final_flush_error_rolls_back_before_retry() {
    failed_append_rolls_back(usize::MAX, true, false);
}

// Release a blocked worker even when an assertion fails before the handshake.
struct Release(Option<mpsc::Sender<()>>);

impl Release {
    fn send(&mut self) {
        if let Some(tx) = self.0.take() {
            tx.send(()).ok();
        }
    }
}

impl Drop for Release {
    fn drop(&mut self) {
        self.send();
    }
}

#[test]
fn slow_writer_keeps_adjacent_batches_and_flush_barriers_ordered() {
    let root = tempfile::tempdir().unwrap();
    let (entered, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let batches = Arc::new(Mutex::new(Vec::new()));
    let recorded = batches.clone();
    let mut first = true;
    let writer = EventWriter::with_append(root.path().into(), move |dir, batch| {
        if first {
            first = false;
            entered.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        if !batch.is_empty() {
            recorded.lock().unwrap().push(reference(batch));
        }
        append_batch(dir, batch)
    })
    .unwrap();
    let mut release = Release(Some(release));
    writer.enqueue(Logged::User("first".into())).unwrap();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let preceding = records(256);
    for record in &preceding {
        writer.enqueue(record.clone()).unwrap();
    }
    let (reply, flushed) = mpsc::sync_channel(1);
    writer.commands.send(WriteCommand::Flush(reply)).unwrap();
    writer
        .enqueue(Logged::User("after barrier".into()))
        .unwrap();
    release.send();
    flushed
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    writer.flush().unwrap();
    assert_eq!(
        *batches.lock().unwrap(),
        [
            reference(&[Logged::User("first".into())]),
            reference(&preceding),
            reference(&[Logged::User("after barrier".into())])
        ]
    );
    let mut expected = vec![Logged::User("first".into())];
    expected.extend(preceding);
    expected.push(Logged::User("after barrier".into()));
    assert_eq!(
        std::fs::read(root.path().join("events.jsonl")).unwrap(),
        reference(&expected)
    );
}

#[test]
fn failed_writer_retains_pressure_records_and_recovers_once() {
    let root = tempfile::tempdir().unwrap();
    let broken = root.path().join("events.jsonl");
    std::fs::create_dir(&broken).unwrap();
    let writer = EventWriter::new(root.path().into()).unwrap();
    let records = records(4_000);
    writer.enqueue(records[0].clone()).unwrap();
    assert!(writer.flush().is_err());
    let failures = writer.failures();
    assert!(failures.try_recv().is_ok());
    for record in &records[1..] {
        writer.enqueue(record.clone()).unwrap();
    }
    assert!(writer.flush().is_err());
    assert!(writer.has_error());
    assert!(failures.try_recv().is_err());
    std::fs::remove_dir(&broken).unwrap();
    writer.flush().unwrap();
    assert!(!writer.has_error());
    assert_eq!(std::fs::read(&broken).unwrap(), reference(&records));
    drop(writer);
    assert_eq!(std::fs::read(&broken).unwrap(), reference(&records));
}

#[test]
fn drop_retries_and_drains_records_after_storage_recovers() {
    let root = tempfile::tempdir().unwrap();
    let broken = root.path().join("events.jsonl");
    std::fs::create_dir(&broken).unwrap();
    let writer = EventWriter::new(root.path().into()).unwrap();
    let records = records(4_000);
    writer.enqueue(records[0].clone()).unwrap();
    assert!(writer.flush().is_err());
    for record in &records[1..] {
        writer.enqueue(record.clone()).unwrap();
    }
    assert!(writer.flush().is_err());
    std::fs::remove_dir(&broken).unwrap();
    drop(writer);
    assert_eq!(std::fs::read(&broken).unwrap(), reference(&records));
}

#[test]
fn blocked_session_writer_does_not_block_another_session() {
    let root = tempfile::tempdir().unwrap();
    let (entered, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut first = true;
    let blocked = EventWriter::with_append(root.path().join("blocked"), move |dir, batch| {
        if first {
            first = false;
            entered.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        append_batch(dir, batch)
    })
    .unwrap();
    let other_dir = root.path().join("other");
    let other = EventWriter::new(other_dir.clone()).unwrap();
    let mut release = Release(Some(release));
    blocked.enqueue(Logged::User("blocked".into())).unwrap();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    other.enqueue(Logged::User("independent".into())).unwrap();
    let (reply, flushed) = mpsc::sync_channel(1);
    other.commands.send(WriteCommand::Flush(reply)).unwrap();
    flushed
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(
        std::fs::read(other_dir.join("events.jsonl")).unwrap(),
        reference(&[Logged::User("independent".into())])
    );
    release.send();
    blocked.flush().unwrap();
}

#[test]
#[ignore = "manual offline persistence-pressure diagnostic"]
fn gated_writer_pressure_probe() {
    let root = tempfile::tempdir().unwrap();
    let (entered, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let batches = Arc::new(Mutex::new(Vec::new()));
    let observed = batches.clone();
    let mut first = true;
    let writer = EventWriter::with_append(root.path().into(), move |dir, batch| {
        if first {
            first = false;
            entered.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(30)).unwrap();
        }
        if !batch.is_empty() {
            let start = Instant::now();
            append_batch(dir, batch)?;
            let elapsed = start.elapsed();
            observed.lock().unwrap().push((batch.len(), elapsed));
        }
        Ok(())
    })
    .unwrap();
    let mut release = Release(Some(release));
    writer.enqueue(Logged::User("gate".into())).unwrap();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let payload = "x".repeat(32 * 1024);
    let queued: Vec<_> = (0..256)
        .map(|n| Logged::User(format!("{n}:{payload}")))
        .collect();
    let serialized_bytes = reference(&queued).len();
    let start = Instant::now();
    for record in queued {
        writer.enqueue(record).unwrap();
    }
    let enqueue = start.elapsed();
    let start = Instant::now();
    release.send();
    writer.flush().unwrap();
    let flush = start.elapsed();
    eprintln!(
        "persistence_pressure={}",
        serde_json::json!({
            "enqueue_ms": enqueue.as_secs_f64() * 1000.,
            "flush_after_release_ms": flush.as_secs_f64() * 1000.,
            "queued_serialized_bytes": serialized_bytes,
            "batches": batches.lock().unwrap().iter().map(|(records, elapsed)| {
                serde_json::json!({
                    "records": records,
                    "append_ms": elapsed.as_secs_f64() * 1000.,
                })
            }).collect::<Vec<_>>(),
        })
    );
}
