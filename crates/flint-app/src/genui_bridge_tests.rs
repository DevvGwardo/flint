use super::*;
use flint_agent::TaskOutcome;
use flint_agent::genui::{Action, CATALOG_ID, Component, SURFACE_ID, TextValue};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn python() -> &'static Path {
    static PYTHON: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    PYTHON.get_or_init(|| {
        // /usr/bin/python3 is an xcrun shim on macOS. Resolve its trusted
        // interpreter so the shim cannot inject SDK variables into env tests.
        let output = std::process::Command::new("/usr/bin/python3")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let path = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
        assert!(path.is_absolute() && path.is_file());
        path
    })
}

fn input(uid: u64, text: &str) -> SnapshotInput {
    SnapshotInput {
        session_uid: uid,
        task: Some(TaskMetadata {
            id: 3,
            contract_revision: 4,
            work_revision: 5,
            outcome: TaskOutcome::NeedsReview,
        }),
        surface: SurfaceSnapshot {
            surface_id: SURFACE_ID.into(),
            catalog_id: CATALOG_ID.into(),
            components: vec![
                Component::Button {
                    id: "root".into(),
                    child: "label".into(),
                    action: Action {
                        name: ActionName::OpenTask,
                    },
                },
                Component::PlainText {
                    id: "label".into(),
                    text: TextValue::Literal(text.into()),
                    style: None,
                },
            ],
            data: BTreeMap::from([("value".into(), text.into())]),
        },
    }
}

struct Harness {
    dir: TempDir,
    root: PathBuf,
    executable: PathBuf,
}

impl Harness {
    fn new(mode: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("gb-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        let root = dir.path().join("r");
        let executable = dir.path().join("child.py");
        let report = serde_json::to_string(&dir.path().join("report.json")).unwrap();
        let marker = serde_json::to_string(&dir.path().join("escaped")).unwrap();
        let mode = serde_json::to_string(mode).unwrap();
        let python = python().display();
        let script = format!(
            r#"#!{python}
import json, os, socket, struct, subprocess, sys, time
MODE = {mode}
REPORT = {report}
MARKER = {marker}
report = {{"keys": sorted(os.environ), "pid": os.getpid(), "snapshots": []}}
def save():
    with open(REPORT + ".tmp", "w") as f:
        json.dump(report, f)
    os.replace(REPORT + ".tmp", REPORT)
save()
if MODE == "boot_stall":
    time.sleep(30)
if MODE == "descendant":
    code = "import time; time.sleep(2); open(" + repr(MARKER) + ", 'w').write('escaped')"
    descendant = subprocess.Popen([sys.executable, "-c", code])
    report["descendant"] = descendant.pid
    save()
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(os.environ["HOOTER_GENUI_SOCKET"])
g = os.environ["HOOTER_GENUI_GENERATION"]
def send(obj):
    data = json.dumps(obj, separators=(",", ":")).encode()
    s.sendall(struct.pack(">I", len(data)) + data)
def read_n(n):
    data = b""
    while len(data) < n:
        part = s.recv(n - len(data))
        if not part:
            raise EOFError()
        data += part
    return data
def receive():
    return json.loads(read_n(struct.unpack(">I", read_n(4))[0]))
def frame(kind, **values):
    return dict(version=1, kind=kind, generation=g, **values)
if MODE == "hello_stall":
    time.sleep(30)
token = "incorrect" if MODE == "bad_token" else os.environ["HOOTER_GENUI_CAPABILITY"]
send(frame("hello", token=token))
if MODE == "bad_token":
    time.sleep(30)
assert receive()["kind"] == "ready"
first = True
while True:
    message = receive()
    if message["kind"] == "shutdown":
        report["shutdown"] = True
        save()
        sys.exit(0)
    assert message["kind"] == "snapshot"
    seq = message["sequence"]
    report["snapshots"].append({{"sequence": seq, "time": time.monotonic(),
                                "value": message["surface"]["data"]["value"]}})
    save()
    if MODE == "no_ack":
        continue
    send(frame("ack", sequence=seq))
    if MODE == "stale_ack" and seq > 1:
        send(frame("ack", sequence=1))
    if not first:
        continue
    first = False
    if MODE == "crash":
        os._exit(7)
    if MODE == "partial_stall":
        s.sendall(b"\x00")
        time.sleep(30)
    if MODE == "oversize":
        s.sendall(struct.pack(">I", 262145))
        time.sleep(30)
    if MODE == "unknown_ack":
        send(frame("ack", sequence=seq+1))
    if MODE == "wrong_direction":
        send(frame("ready"))
    if MODE == "flood":
        for i in range(200):
            send(frame("ack", sequence=seq))
    if MODE == "action_flood":
        for i in range(64):
            send(frame("action", sequence=seq, request=i+1, source="root", name="open_task"))
    if MODE == "two_actions":
        for i in range(2):
            send(frame("action", sequence=seq, request=i+1, source="root", name="open_task"))
    if MODE in ("action", "replay", "wrong_source", "wrong_name",
                "wrong_sequence", "wrong_generation"):
        action = frame("action", sequence=seq, request=1,
                       source="root", name="open_task")
        if MODE == "wrong_source":
            action["source"] = "label"
        if MODE == "wrong_name":
            action["name"] = "stage_prompt"
        if MODE == "wrong_sequence":
            action["sequence"] += 1
        if MODE == "wrong_generation":
            action["generation"] = "different"
        send(action)
        if MODE == "replay":
            send(action)
"#
        );
        fs::write(&executable, script).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            dir,
            root,
            executable,
        }
    }

    fn launch(&self) -> Bridge {
        Bridge::launch(
            &tokio::runtime::Handle::current(),
            &self.root,
            &self.executable,
            input(7, "initial"),
        )
        .unwrap()
    }

    fn report(&self) -> Option<serde_json::Value> {
        fs::read(self.dir.path().join("report.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    }

    async fn wait_report(
        &self,
        predicate: impl Fn(&serde_json::Value) -> bool,
    ) -> serde_json::Value {
        timeout(Duration::from_secs(8), async {
            loop {
                if let Some(report) = self.report()
                    && predicate(&report)
                {
                    return report;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fake child report timed out")
    }

    fn launch_directories(&self) -> Vec<PathBuf> {
        fs::read_dir(&self.root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect()
    }
}

async fn event(bridge: &Bridge) -> BridgeEvent {
    timeout(Duration::from_secs(15), bridge.events.recv())
        .await
        .expect("bridge event timed out")
        .expect("event channel closed")
}

async fn disconnected(bridge: &Bridge) -> &'static str {
    loop {
        match event(bridge).await {
            BridgeEvent::Disconnected(reason) => return reason,
            BridgeEvent::Connected | BridgeEvent::Action(_) => {}
        }
    }
}

async fn action(bridge: &Bridge) -> BridgeAction {
    loop {
        match event(bridge).await {
            BridgeEvent::Connected => {}
            BridgeEvent::Action(action) => return action,
            BridgeEvent::Disconnected(reason) => panic!("disconnected: {reason}"),
        }
    }
}

#[tokio::test]
async fn real_owned_child_roundtrip_and_private_socket() {
    let harness = Harness::new("action");
    let bridge = harness.launch();
    let action = action(&bridge).await;
    assert!(bridge.is_connected());
    assert_eq!(
        bridge.authorize(&action, 7, input(7, "initial").task.as_ref()),
        Ok(ActionName::OpenTask)
    );
    assert!(bridge.authorize(&action, 7, action.task.as_ref()).is_err());
    assert_eq!(fs::metadata(&harness.root).unwrap().mode() & 0o777, 0o700);
    let launches = harness.launch_directories();
    assert_eq!(launches.len(), 1);
    assert_eq!(fs::metadata(&launches[0]).unwrap().mode() & 0o777, 0o700);
    assert_eq!(
        fs::metadata(launches[0].join("s")).unwrap().mode() & 0o777,
        0o600
    );
    bridge.shutdown();
    assert!(!bridge.is_connected());
    disconnected(&bridge).await;
    assert!(harness.launch_directories().is_empty());
    assert!(harness.report().unwrap()["shutdown"].as_bool().unwrap());
}

#[tokio::test]
async fn child_environment_is_an_explicit_secret_free_whitelist() {
    let harness = Harness::new("idle");
    let bridge = harness.launch();
    assert!(matches!(event(&bridge).await, BridgeEvent::Connected));
    let report = harness.wait_report(|_| true).await;
    let permitted = [
        "PATH",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        // CoreFoundation can synthesize this after exec on macOS. It is not
        // forwarded from the native parent's environment.
        "__CF_USER_TEXT_ENCODING",
        "HOOTER_GENUI_SOCKET",
        "HOOTER_GENUI_GENERATION",
        "HOOTER_GENUI_CAPABILITY",
    ];
    for key in report["keys"].as_array().unwrap() {
        assert!(permitted.contains(&key.as_str().unwrap()), "{key}");
    }
    let token = "private-capability-do-not-log";
    let mut command = command(
        Path::new("/usr/bin/true"),
        Path::new("/tmp"),
        "generation",
        token,
    );
    let std = command.as_std_mut();
    assert_eq!(std.get_program(), "/usr/bin/true");
    assert_eq!(std.get_args().count(), 0);
    let keys: Vec<_> = std
        .get_envs()
        .map(|(key, _)| key.to_str().unwrap())
        .collect();
    assert!(!keys.iter().any(|key| key.starts_with("DYLD_")));
    assert!(!keys.contains(&"OPENAI_API_KEY"));
    bridge.shutdown();
    disconnected(&bridge).await;
}

#[tokio::test]
async fn authentication_failure_is_closed_without_secret_in_diagnostics() {
    let harness = Harness::new("bad_token");
    let bridge = harness.launch();
    let reason = disconnected(&bridge).await;
    assert_eq!(reason, "GenUI authentication failed.");
    assert!(!reason.contains("incorrect"));
    assert!(!bridge.is_connected());
    assert!(bridge.publish(input(8, "later")).is_err());
    assert!(harness.launch_directories().is_empty());
}

#[tokio::test]
async fn peer_uid_and_pid_must_match_the_owned_child() {
    // macOS does not expose LOCAL_PEERPID for an anonymous socketpair.
    let harness = Harness::new("idle");
    let socket = harness.dir.path().join("p");
    let listener = UnixListener::bind(&socket).unwrap();
    let (connected, accepted) = tokio::join!(UnixStream::connect(&socket), listener.accept());
    let _connected = connected.unwrap();
    let (left, _) = accepted.unwrap();
    assert!(verify_peer(&left, uid().wrapping_add(1), Some(std::process::id())).is_err());
    assert!(verify_peer(&left, uid(), Some(std::process::id())).is_ok());
    if left.peer_cred().unwrap().pid().is_some() {
        assert!(verify_peer(&left, uid(), Some(std::process::id().wrapping_add(1))).is_err());
    }
}

#[tokio::test]
async fn foreground_rechecks_all_captured_target_fields_and_replay() {
    let harness = Harness::new("action");
    let bridge = harness.launch();
    let captured = action(&bridge).await;
    assert_eq!(captured.session_uid, 7);
    assert!(
        bridge
            .authorize(&captured, 8, captured.task.as_ref())
            .is_err()
    );
    assert!(bridge.authorize(&captured, 7, None).is_err());
    for field in ["id", "contract_revision", "work_revision", "outcome"] {
        let mut task = captured.task.clone().unwrap();
        match field {
            "id" => task.id += 1,
            "contract_revision" => task.contract_revision += 1,
            "work_revision" => task.work_revision += 1,
            "outcome" => task.outcome = TaskOutcome::Accepted,
            _ => unreachable!(),
        }
        assert!(bridge.authorize(&captured, 7, Some(&task)).is_err());
    }
    let mut candidates = Vec::new();
    let mut changed = captured.clone();
    changed.generation = "different".into();
    candidates.push(changed);
    let mut changed = captured.clone();
    changed.sequence += 1;
    candidates.push(changed);
    let mut changed = captured.clone();
    changed.source = "label".into();
    candidates.push(changed);
    let mut changed = captured.clone();
    changed.name = ActionName::StagePrompt;
    candidates.push(changed);
    let mut changed = captured.clone();
    changed.request = 0;
    candidates.push(changed);
    let mut changed = captured.clone();
    changed.request = MAX_SAFE_INTEGER + 1;
    candidates.push(changed);
    let mut changed = captured.clone();
    changed.request += 1;
    candidates.push(changed);
    let mut changed = captured.clone();
    changed.task.as_mut().unwrap().work_revision += 1;
    candidates.push(changed);
    for candidate in candidates {
        assert!(
            bridge
                .authorize(&candidate, 7, captured.task.as_ref())
                .is_err()
        );
    }
    assert_eq!(
        bridge.authorize(&captured, 7, captured.task.as_ref()),
        Ok(ActionName::OpenTask)
    );
    assert!(
        bridge
            .authorize(&captured, 7, captured.task.as_ref())
            .is_err()
    );
    bridge.shutdown();
    disconnected(&bridge).await;
}

#[tokio::test]
async fn publish_invalidates_queued_actions_before_io_and_even_after_switching_back() {
    let harness = Harness::new("action");
    let bridge = harness.launch();
    let captured = action(&bridge).await;
    bridge.publish(input(8, "other")).unwrap();
    assert!(
        bridge
            .authorize(&captured, 7, captured.task.as_ref())
            .is_err()
    );
    bridge.publish(input(7, "initial")).unwrap();
    assert!(
        bridge
            .authorize(&captured, 7, captured.task.as_ref())
            .is_err()
    );
    bridge.shutdown();
    disconnected(&bridge).await;
}

#[tokio::test]
async fn focus_boundary_revokes_a_queued_click_without_any_intermediate_frame() {
    let harness = Harness::new("action");
    let bridge = harness.launch();
    let captured = action(&bridge).await;
    bridge.invalidate_actions().unwrap();
    bridge.publish(input(7, "initial")).unwrap();
    assert!(bridge.authorize(&captured, 7, captured.task.as_ref()).is_err());
    bridge.shutdown();
    disconnected(&bridge).await;
}

#[tokio::test]
async fn malformed_publish_does_not_replace_the_authoritative_snapshot() {
    let harness = Harness::new("action");
    let bridge = harness.launch();
    let captured = action(&bridge).await;
    let mut invalid = input(8, "invalid");
    invalid.surface.components.push(Component::Card {
        id: "reused".into(),
        child: "label".into(),
    });
    assert!(bridge.publish(invalid).is_err());
    let mut invalid = input(8, "invalid");
    invalid.task.as_mut().unwrap().id = MAX_SAFE_INTEGER + 1;
    assert!(bridge.publish(invalid).is_err());
    assert_eq!(
        bridge.authorize(&captured, 7, captured.task.as_ref()),
        Ok(ActionName::OpenTask)
    );
    bridge.shutdown();
    disconnected(&bridge).await;
}

#[tokio::test]
async fn hostile_ingress_fails_closed_for_generation_sequence_source_name_and_direction() {
    for mode in [
        "wrong_source",
        "wrong_name",
        "wrong_sequence",
        "wrong_generation",
        "wrong_direction",
        "unknown_ack",
        "replay",
        "flood",
        "oversize",
    ] {
        let harness = Harness::new(mode);
        let bridge = harness.launch();
        let reason = disconnected(&bridge).await;
        assert!(!bridge.is_connected(), "{mode}: {reason}");
        assert!(bridge.publish(input(8, "after failure")).is_err());
        assert!(harness.launch_directories().is_empty());
    }
}

#[tokio::test]
async fn bounded_action_queue_reserves_disconnect_and_invalidates_every_queued_action() {
    let harness = Harness::new("action_flood");
    let bridge = harness.launch();
    timeout(Duration::from_secs(5), async {
        loop {
            if bridge.shared.state.lock().unwrap().closed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let mut actions = 0;
    loop {
        match event(&bridge).await {
            BridgeEvent::Connected => {}
            BridgeEvent::Action(action) => {
                actions += 1;
                assert!(bridge.authorize(&action, 7, action.task.as_ref()).is_err());
            }
            BridgeEvent::Disconnected(reason) => {
                assert_eq!(reason, "GenUI event queue overflow.");
                break;
            }
        }
    }
    assert!(actions > 0 && actions < EVENT_CAPACITY);
    assert!(harness.launch_directories().is_empty());
}

#[tokio::test]
async fn legitimate_increasing_nonces_authorize_once_in_order() {
    let harness = Harness::new("two_actions");
    let bridge = harness.launch();
    let first = action(&bridge).await;
    let second = action(&bridge).await;
    assert_eq!(
        bridge.authorize(&first, 7, first.task.as_ref()),
        Ok(ActionName::OpenTask)
    );
    assert_eq!(
        bridge.authorize(&second, 7, second.task.as_ref()),
        Ok(ActionName::OpenTask)
    );
    assert!(bridge.authorize(&first, 7, first.task.as_ref()).is_err());
    assert!(bridge.authorize(&second, 7, second.task.as_ref()).is_err());
    bridge.shutdown();
    disconnected(&bridge).await;
}

#[tokio::test]
async fn coalesces_bursts_and_limits_snapshot_rate_with_legitimate_stale_acks() {
    let harness = Harness::new("stale_ack");
    let bridge = harness.launch();
    assert!(matches!(event(&bridge).await, BridgeEvent::Connected));
    harness
        .wait_report(|report| !report["snapshots"].as_array().unwrap().is_empty())
        .await;
    for number in 0..200 {
        bridge
            .publish(input(7, &format!("burst-{number}")))
            .unwrap();
    }
    let report = harness
        .wait_report(|report| {
            report["snapshots"]
                .as_array()
                .unwrap()
                .last()
                .is_some_and(|last| last["value"] == "burst-199")
        })
        .await;
    assert_eq!(report["snapshots"].as_array().unwrap().len(), 2);
    for number in 0..30 {
        bridge.publish(input(7, &format!("live-{number}"))).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let report = harness
        .wait_report(|report| {
            report["snapshots"]
                .as_array()
                .unwrap()
                .last()
                .is_some_and(|last| last["value"] == "live-29")
        })
        .await;
    let snapshots = report["snapshots"].as_array().unwrap();
    assert!(snapshots.len() < 15);
    for pair in snapshots.windows(2) {
        let gap = pair[1]["time"].as_f64().unwrap() - pair[0]["time"].as_f64().unwrap();
        assert!(gap >= 0.025, "unbounded snapshot rate: {gap}");
        assert!(pair[1]["sequence"].as_u64().unwrap() > pair[0]["sequence"].as_u64().unwrap());
    }
    assert!(bridge.is_connected());
    bridge.shutdown();
    disconnected(&bridge).await;
}

#[tokio::test]
async fn idle_reads_are_not_disconnected_by_frame_or_ack_deadlines() {
    let harness = Harness::new("idle");
    let bridge = harness.launch();
    assert!(matches!(event(&bridge).await, BridgeEvent::Connected));
    harness
        .wait_report(|report| !report["snapshots"].as_array().unwrap().is_empty())
        .await;
    tokio::time::sleep(ACK_TIMEOUT + Duration::from_millis(300)).await;
    assert!(bridge.is_connected());
    assert!(bridge.events.is_empty());
    bridge.shutdown();
    disconnected(&bridge).await;
}

#[tokio::test]
async fn unacknowledged_snapshot_and_partial_frame_have_bounded_waits() {
    for (mode, expected) in [
        ("no_ack", "GenUI snapshot acknowledgement timed out."),
        ("partial_stall", "GenUI partial frame timed out."),
        ("hello_stall", "GenUI boot or handshake timed out."),
    ] {
        let harness = Harness::new(mode);
        let bridge = harness.launch();
        assert_eq!(disconnected(&bridge).await, expected);
        assert!(harness.launch_directories().is_empty());
    }
}

#[tokio::test]
async fn drop_during_boot_cancels_and_cleans_only_owned_launch_directory() {
    let harness = Harness::new("boot_stall");
    let bridge = harness.launch();
    let receiver = bridge.events.clone();
    fs::write(harness.root.join("unrelated"), "preserve").unwrap();
    harness.wait_report(|_| true).await;
    drop(bridge);
    assert!(matches!(
        timeout(Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap(),
        BridgeEvent::Disconnected(_)
    ));
    assert_eq!(
        fs::read_to_string(harness.root.join("unrelated")).unwrap(),
        "preserve"
    );
    assert_eq!(
        harness.launch_directories(),
        vec![harness.root.join("unrelated")]
    );
}

#[tokio::test]
async fn crash_and_parent_drop_clean_child_descendants_and_socket() {
    let harness = Harness::new("crash");
    let bridge = harness.launch();
    disconnected(&bridge).await;
    assert!(harness.launch_directories().is_empty());
    let harness = Harness::new("descendant");
    let mut unrelated = Command::new(python());
    unrelated
        .args(["-c", "import time; time.sleep(2)"])
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut unrelated = unrelated.spawn().unwrap();
    let bridge = harness.launch();
    let receiver = bridge.events.clone();
    assert!(matches!(event(&bridge).await, BridgeEvent::Connected));
    harness
        .wait_report(|report| report["descendant"].is_number())
        .await;
    drop(bridge);
    assert!(matches!(
        timeout(Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap(),
        BridgeEvent::Disconnected(_)
    ));
    assert!(harness.launch_directories().is_empty());
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(!harness.dir.path().join("escaped").exists());
    assert!(unrelated.wait().await.unwrap().success());
}

#[tokio::test]
async fn runtime_teardown_kills_owned_child_and_marks_foreground_closed() {
    let harness = Harness::new("boot_stall");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let bridge = Bridge::launch(
        runtime.handle(),
        &harness.root,
        &harness.executable,
        input(7, "initial"),
    )
    .unwrap();
    // The worker need not be polled even once for its owned guards to drop.
    runtime.shutdown_background();
    assert!(!bridge.is_connected());
    assert!(bridge.publish(input(8, "after runtime drop")).is_err());
    assert!(harness.launch_directories().is_empty());
}

#[tokio::test]
async fn frame_reader_handles_fragmentation_and_rejects_invalid_lengths_and_truncation() {
    let hello = Envelope::Hello {
        version: BRIDGE_VERSION,
        generation: "test".into(),
        token: "private".into(),
    };
    let bytes = hello.encode().unwrap();
    let mut encoded = (bytes.len() as u32).to_be_bytes().to_vec();
    encoded.extend(bytes);
    let (mut left, mut right) = tokio::io::duplex(32);
    let send = tokio::spawn(async move {
        for byte in encoded {
            left.write_all(&[byte]).await.unwrap();
            tokio::task::yield_now().await;
        }
    });
    assert_eq!(read_frame(&mut right).await.unwrap(), hello);
    send.await.unwrap();
    for bytes in [
        vec![0, 0, 0, 0],
        ((MAX_FRAME_BYTES + 1) as u32).to_be_bytes().to_vec(),
        vec![0, 0],
        vec![0, 0, 0, 10, b'{'],
        vec![0, 0, 0, 1, 0xff],
        vec![0, 0, 0, 2, b'{', b'}'],
    ] {
        assert!(read_frame(&mut bytes.as_slice()).await.is_err());
    }
}

#[tokio::test]
async fn write_backpressure_is_bounded_and_first_byte_idle_is_cancellable() {
    let (mut writer, _reader) = tokio::io::duplex(1);
    let envelope = Envelope::Ready {
        version: BRIDGE_VERSION,
        generation: "test".into(),
    };
    assert_eq!(
        write_frame(&mut writer, &envelope).await,
        Err("GenUI frame write timed out.")
    );
    let (_writer, mut reader) = tokio::io::duplex(1);
    assert!(
        timeout(Duration::from_millis(30), read_frame(&mut reader))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn runtime_roots_and_executable_paths_fail_closed_without_changing_permissions() {
    let harness = Harness::new("idle");
    fs::create_dir(&harness.root).unwrap();
    fs::set_permissions(&harness.root, fs::Permissions::from_mode(0o755)).unwrap();
    let launch = || {
        Bridge::launch(
            &tokio::runtime::Handle::current(),
            &harness.root,
            &harness.executable,
            input(7, "initial"),
        )
    };
    assert!(launch().is_err());
    assert_eq!(fs::metadata(&harness.root).unwrap().mode() & 0o777, 0o755);
    fs::set_permissions(&harness.root, fs::Permissions::from_mode(0o700)).unwrap();
    let link = harness.dir.path().join("link");
    std::os::unix::fs::symlink(&harness.root, &link).unwrap();
    assert!(
        Bridge::launch(
            &tokio::runtime::Handle::current(),
            &link,
            &harness.executable,
            input(7, "initial"),
        )
        .is_err()
    );
    assert!(
        Bridge::launch(
            &tokio::runtime::Handle::current(),
            &harness.root,
            Path::new("relative-executable"),
            input(7, "initial"),
        )
        .is_err()
    );
    let long = harness.dir.path().join("x".repeat(90));
    assert!(
        Bridge::launch(
            &tokio::runtime::Handle::current(),
            &long,
            &harness.executable,
            input(7, "initial"),
        )
        .is_err()
    );
    assert!(fs::read_dir(long).unwrap().next().is_none());
}

#[test]
fn debug_and_capability_comparison_do_not_expose_private_material() {
    assert!(secret_matches("identical", "identical"));
    assert!(!secret_matches("identical", "different"));
    assert!(!secret_matches("identical", "identical-longer"));
    let action = BridgeAction {
        generation: "private-generation".into(),
        sequence: 1,
        request: 1,
        source: "private-source".into(),
        name: ActionName::OpenTask,
        session_uid: 7,
        task: input(7, "initial").task,
    };
    let debug = format!("{:?}", BridgeEvent::Action(action));
    assert!(!debug.contains("private"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[ignore = "Requires the built Flutter macOS companion and a GUI session"]
async fn real_flutter_companion_mounts_two_native_snapshots_and_optionally_returns_action() {
    let executable = crate::genui_ui::companion_executable().expect("build companion");
    let home = PathBuf::from(std::env::var_os("HOME").expect("home"));
    let root = home.join(".flint/genui-runtime");
    let mut second = input(7, "Rust host frame 2: native action smoke");
    // This path deliberately uses the production sandbox-authorized runtime
    // root and real peer PID check, not the Python test renderer.
    let bridge = Bridge::launch(&tokio::runtime::Handle::current(), &root, &executable, input(7, "Rust host frame 1")).unwrap();
    async fn wait_rendered(bridge: &Bridge) {
        tokio::time::timeout(Duration::from_secs(15), async {
            while !bridge.is_current_rendered() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await.expect("SDK frame was not mounted and acknowledged");
    }
    wait_rendered(&bridge).await;
    eprintln!("Real Flutter acknowledged Rust host frame 1.");
    second.task.as_mut().unwrap().work_revision += 1;
    bridge.publish(second.clone()).unwrap();
    wait_rendered(&bridge).await;
    eprintln!("Real Flutter acknowledged Rust host frame 2.");
    if std::env::var_os("HOOTER_GENUI_SMOKE_ACTION").is_some() {
        eprintln!("Waiting up to 90 seconds for the fixture button.");
        let action = tokio::time::timeout(Duration::from_secs(90), async {
            loop {
                match bridge.events.recv().await.unwrap() {
                    BridgeEvent::Action(action) => break action,
                    BridgeEvent::Disconnected(error) => panic!("{error}"),
                    BridgeEvent::Connected => {}
                }
            }
        }).await.expect("No SDK action received");
        assert_eq!(bridge.authorize(&action, 7, second.task.as_ref()).unwrap(), ActionName::OpenTask);
        assert!(bridge.authorize(&action, 7, second.task.as_ref()).is_err());
        eprintln!("Real SDK action authorized exactly once against the current native target.");
    }
    bridge.shutdown();
    tokio::time::sleep(Duration::from_millis(150)).await;
}
