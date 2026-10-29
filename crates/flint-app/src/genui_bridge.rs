//! Owned, credential-free renderer process. Only the native foreground may
//! authorize an action; the IO worker never mutates application state.

use std::path::Path;
use std::sync::{Arc, Mutex};

use flint_agent::genui::{
    ActionName, BRIDGE_VERSION, Envelope, MAX_SAFE_INTEGER, SurfaceSnapshot, TaskMetadata,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SnapshotInput {
    pub session_uid: u64,
    pub task: Option<TaskMetadata>,
    pub surface: SurfaceSnapshot,
}

#[derive(Clone)]
pub(crate) struct BridgeAction {
    generation: String,
    sequence: u64,
    request: u64,
    source: String,
    name: ActionName,
    pub session_uid: u64,
    pub task: Option<TaskMetadata>,
}

impl std::fmt::Debug for BridgeAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BridgeAction")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) enum BridgeEvent {
    Connected,
    Action(BridgeAction),
    Disconnected(&'static str),
}

struct State {
    generation: String,
    sequence: u64,
    applied_sequence: u64,
    latest: SnapshotInput,
    received_request: u64,
    authorized_request: u64,
    connected: bool,
    closed: bool,
}

struct Shared {
    state: Mutex<State>,
    stop: CancellationToken,
}

pub(crate) struct Bridge {
    shared: Arc<Shared>,
    pub events: async_channel::Receiver<BridgeEvent>,
}

impl Bridge {
    /// Launches only an absolute executable in its own process group. A
    /// successful return means "booting"; wait for Connected before use.
    pub fn launch(
        runtime: &tokio::runtime::Handle,
        runtime_root: &Path,
        executable: &Path,
        initial: SnapshotInput,
    ) -> Result<Self, &'static str> {
        #[cfg(unix)]
        {
            unix::launch(runtime, runtime_root, executable, initial)
        }
        #[cfg(not(unix))]
        {
            let _ = (runtime, runtime_root, executable, initial);
            Err("GenUI requires a Unix-domain socket on this platform.")
        }
    }

    /// Updates the authoritative target synchronously, not when the coalesced
    /// snapshot is eventually written. This immediately invalidates old clicks.
    pub fn publish(&self, input: SnapshotInput) -> Result<(), &'static str> {
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| "GenUI state failed.")?;
        if state.closed {
            return Err("GenUI is closed.");
        }
        if state.latest == input {
            return Ok(());
        }
        let sequence = state
            .sequence
            .checked_add(1)
            .filter(|value| *value <= MAX_SAFE_INTEGER)
            .ok_or("GenUI sequence exhausted.")?;
        snapshot(&state.generation, sequence, &input).encode()?;
        state.sequence = sequence;
        state.latest = input;
        Ok(())
    }

    pub fn shutdown(&self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.closed = true;
            state.connected = false;
        }
        self.shared.stop.cancel();
    }

    /// Invalidate queued clicks at a focus boundary even if no frame renders
    /// between switching away and back to the same native task.
    pub fn invalidate_actions(&self) -> Result<(), &'static str> {
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| "GenUI state failed.")?;
        if state.closed {
            return Err("GenUI is closed.");
        }
        state.sequence = state
            .sequence
            .checked_add(1)
            .filter(|sequence| *sequence <= MAX_SAFE_INTEGER)
            .ok_or("GenUI sequence exhausted.")?;
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.shared
            .state
            .lock()
            .is_ok_and(|state| state.connected && !state.closed)
    }

    /// Renderer acknowledgement is sent only after the SDK mounted the frame.
    pub fn is_current_rendered(&self) -> bool {
        self.shared.state.lock().is_ok_and(|state| {
            state.connected && !state.closed && state.applied_sequence == state.sequence
        })
    }

    /// Must be called on the native foreground with the actual current target.
    /// A queued event is not authority, even if it passed ingress checks.
    pub fn authorize(
        &self,
        action: &BridgeAction,
        current_uid: u64,
        current_task: Option<&TaskMetadata>,
    ) -> Result<ActionName, &'static str> {
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| "GenUI state failed.")?;
        if state.closed
            || !state.connected
            || action.generation != state.generation
            || action.sequence != state.sequence
            || action.session_uid != state.latest.session_uid
            || action.session_uid != current_uid
            || action.task != state.latest.task
            || action.task.as_ref() != current_task
            || action.request == 0
            || action.request > MAX_SAFE_INTEGER
            || action.request > state.received_request
            || action.request <= state.authorized_request
            || state.latest.surface.action(&action.source) != Some(action.name)
        {
            return Err("Stale or unauthorized GenUI action.");
        }
        state.authorized_request = action.request;
        Ok(action.name)
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn snapshot(generation: &str, sequence: u64, input: &SnapshotInput) -> Envelope {
    Envelope::Snapshot {
        version: BRIDGE_VERSION,
        generation: generation.to_owned(),
        sequence,
        task: input.task.clone(),
        surface: input.surface.clone(),
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use flint_agent::genui::MAX_FRAME_BYTES;
    use flint_platform::{ProcessTree, spawn};
    use std::collections::VecDeque;
    use std::fs::{self, DirBuilder, File};
    use std::io::Read;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    use std::os::unix::net::UnixListener as StdListener;
    use std::process::Stdio;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
    use tokio::net::{UnixListener, UnixStream};
    use tokio::process::{Child, Command};
    use tokio::time::{Instant, timeout};

    const BOOT_TIMEOUT: Duration = Duration::from_secs(10);
    const FRAME_TIMEOUT: Duration = Duration::from_secs(2);
    const ACK_TIMEOUT: Duration = Duration::from_secs(5);
    const FRAME_INTERVAL: Duration = Duration::from_micros(33_334);
    const CLOSE_GRACE: Duration = Duration::from_millis(300);
    const EVENT_CAPACITY: usize = 32;

    unsafe extern "C" {
        fn geteuid() -> u32;
    }

    fn uid() -> u32 {
        // POSIX geteuid has no arguments and no failure mode.
        unsafe { geteuid() }
    }

    fn random_secret() -> Result<String, &'static str> {
        let mut bytes = [0_u8; 32];
        File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut bytes))
            .map_err(|_| "GenUI secure randomness unavailable.")?;
        use std::fmt::Write;
        let mut value = String::with_capacity(64);
        for byte in bytes {
            write!(&mut value, "{byte:02x}").expect("writing to a string cannot fail");
        }
        Ok(value)
    }

    fn private_root(root: &Path) -> Result<(), &'static str> {
        if !root.is_absolute() {
            return Err("GenUI runtime directory must be absolute.");
        }
        match fs::symlink_metadata(root) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                DirBuilder::new()
                    .mode(0o700)
                    .create(root)
                    .map_err(|_| "Cannot create private GenUI runtime directory.")?;
            }
            Err(_) => return Err("Cannot inspect GenUI runtime directory."),
        }
        let metadata =
            fs::symlink_metadata(root).map_err(|_| "Cannot inspect GenUI runtime directory.")?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != uid()
            || metadata.mode() & 0o077 != 0
        {
            return Err("GenUI runtime directory is not private and owner-only.");
        }
        Ok(())
    }

    fn command(executable: &Path, directory: &Path, generation: &str, token: &str) -> Command {
        let mut command = Command::new(executable);
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("TMPDIR", directory)
            .env("HOOTER_GENUI_SOCKET", directory.join("s"))
            .env("HOOTER_GENUI_GENERATION", generation)
            .env("HOOTER_GENUI_CAPABILITY", token)
            .current_dir(directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for key in ["LANG", "LC_ALL", "LC_CTYPE"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
    }

    pub(super) fn launch(
        runtime: &tokio::runtime::Handle,
        runtime_root: &Path,
        executable: &Path,
        initial: SnapshotInput,
    ) -> Result<Bridge, &'static str> {
        let generation = random_secret()?;
        let token = random_secret()?;
        snapshot(&generation, 1, &initial).encode()?;
        if !executable.is_absolute() {
            return Err("GenUI executable must be an absolute release artifact.");
        }
        let executable =
            fs::canonicalize(executable).map_err(|_| "GenUI release executable is missing.")?;
        let metadata =
            fs::metadata(&executable).map_err(|_| "Cannot inspect GenUI release executable.")?;
        if !metadata.is_file() || metadata.mode() & 0o111 == 0 {
            return Err("GenUI release artifact is not executable.");
        }
        private_root(runtime_root)?;
        let directory = tempfile::Builder::new()
            .prefix("g-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(runtime_root)
            .map_err(|_| "Cannot create GenUI launch directory.")?;
        private_root(directory.path())?;
        let socket = directory.path().join("s");
        if socket.as_os_str().len() > 100 {
            return Err("GenUI runtime socket path is too long.");
        }
        let listener =
            StdListener::bind(&socket).map_err(|_| "Cannot bind private GenUI socket.")?;
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))
            .map_err(|_| "Cannot restrict GenUI socket.")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "Cannot configure GenUI socket.")?;
        let _entered = runtime.enter();
        let listener =
            UnixListener::from_std(listener).map_err(|_| "Cannot register GenUI socket.")?;
        let (child, tree) = spawn(&mut command(
            &executable,
            directory.path(),
            &generation,
            &token,
        ))
        .map_err(|_| "Cannot launch GenUI release executable.")?;
        let owned = OwnedChild {
            child,
            tree,
            _directory: directory,
        };
        let (events_tx, events) = async_channel::bounded(EVENT_CAPACITY);
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                generation: generation.clone(),
                sequence: 1,
                applied_sequence: 0,
                latest: initial,
                received_request: 0,
                authorized_request: 0,
                connected: false,
                closed: false,
            }),
            stop: CancellationToken::new(),
        });
        runtime.spawn(run(
            Arc::clone(&shared),
            events_tx,
            listener,
            owned,
            generation,
            token,
            ConnectionGuard(Arc::clone(&shared)),
        ));
        Ok(Bridge { shared, events })
    }

    struct OwnedChild {
        // Signal the group before dropping/reaping the leader, even if the
        // runtime abandons this future. Its PID cannot be reused meanwhile.
        tree: ProcessTree,
        child: Child,
        _directory: TempDir,
    }

    impl OwnedChild {
        async fn close(&mut self) {
            // Always stop descendants, including when the direct child exited.
            tokio::time::sleep(CLOSE_GRACE).await;
            self.tree.terminate();
            tokio::time::sleep(CLOSE_GRACE).await;
            self.tree.kill();
            let _ = timeout(Duration::from_secs(1), self.child.wait()).await;
            self.tree.disarm();
        }
    }

    struct ConnectionGuard(Arc<Shared>);

    impl Drop for ConnectionGuard {
        fn drop(&mut self) {
            if let Ok(mut state) = self.0.state.lock() {
                state.connected = false;
                state.closed = true;
            }
            self.0.stop.cancel();
        }
    }

    async fn run(
        shared: Arc<Shared>,
        events: async_channel::Sender<BridgeEvent>,
        listener: UnixListener,
        mut owned: OwnedChild,
        generation: String,
        token: String,
        guard: ConnectionGuard,
    ) {
        let expected_pid = owned.child.id();
        let handshake = async {
            let (mut stream, _) = listener
                .accept()
                .await
                .map_err(|_| "GenUI connection failed.")?;
            verify_peer(&stream, uid(), expected_pid)?;
            match read_frame(&mut stream).await? {
                Envelope::Hello {
                    generation: received,
                    token: received_token,
                    ..
                } if received == generation && secret_matches(&received_token, &token) => {}
                _ => return Err("GenUI authentication failed."),
            }
            write_frame(
                &mut stream,
                &Envelope::Ready {
                    version: BRIDGE_VERSION,
                    generation: generation.clone(),
                },
            )
            .await?;
            Ok(stream)
        };
        let connection = tokio::select! {
            biased;
            () = shared.stop.cancelled() => Err("GenUI closed."),
            result = timeout(BOOT_TIMEOUT, handshake) =>
                result.unwrap_or(Err("GenUI boot or handshake timed out.")),
        };
        // Bootstrap listener and token are never retained in foreground state.
        drop(listener);
        drop(token);
        let reason = match connection {
            Ok(mut stream) => {
                let result = connected(&shared, &events, &mut stream, &generation).await;
                if let Ok(mut state) = shared.state.lock() {
                    state.connected = false;
                    state.closed = true;
                }
                // The shutdown is bounded even when a child stopped reading.
                let _ = write_frame(
                    &mut stream,
                    &Envelope::Shutdown {
                        version: BRIDGE_VERSION,
                        generation,
                    },
                )
                .await;
                result.err().unwrap_or("GenUI closed.")
            }
            Err(reason) => reason,
        };
        // No queued event can authorize once the connection has failed.
        drop(guard);
        owned.close().await;
        drop(owned);
        let _ = events.try_send(BridgeEvent::Disconnected(reason));
    }

    fn verify_peer(
        stream: &UnixStream,
        expected_uid: u32,
        expected_pid: Option<u32>,
    ) -> Result<(), &'static str> {
        let peer = stream
            .peer_cred()
            .map_err(|_| "Cannot authenticate GenUI peer credentials.")?;
        if peer.uid() != expected_uid
            || peer.pid().is_some_and(|pid| {
                u32::try_from(pid)
                    .ok()
                    .is_none_or(|pid| Some(pid) != expected_pid)
            })
        {
            return Err("GenUI peer credentials do not match the owned child.");
        }
        Ok(())
    }

    fn secret_matches(left: &str, right: &str) -> bool {
        left.len() == right.len()
            && left
                .bytes()
                .zip(right.bytes())
                .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
                == 0
    }

    async fn connected(
        shared: &Arc<Shared>,
        events: &async_channel::Sender<BridgeEvent>,
        stream: &mut UnixStream,
        generation: &str,
    ) -> Result<(), &'static str> {
        {
            let mut state = shared.state.lock().map_err(|_| "GenUI state failed.")?;
            if state.closed {
                return Err("GenUI closed.");
            }
            state.connected = true;
        }
        events
            .try_send(BridgeEvent::Connected)
            .map_err(|_| "GenUI event queue overflow.")?;
        let (mut reader, mut writer) = stream.split();
        // The read future lives across ticks: cancellation never loses a
        // partially consumed frame. Idle first-byte reads have no deadline.
        let mut next_frame = Box::pin(read_frame(&mut reader));
        let mut tick = tokio::time::interval(FRAME_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut sent = VecDeque::new();
        let mut last_sent = 0;
        let mut last_ack = 0;
        let mut pending: Option<(u64, Instant)> = None;
        let mut next_send = Instant::now();
        let mut flood_start = Instant::now();
        let mut flood_count = 0_u32;
        loop {
            tokio::select! {
                biased;
                () = shared.stop.cancelled() => return Ok(()),
                frame = &mut next_frame => {
                    let envelope = frame?;
                    drop(next_frame);
                    next_frame = Box::pin(read_frame(&mut reader));
                    if flood_start.elapsed() >= Duration::from_secs(1) {
                        flood_start = Instant::now();
                        flood_count = 0;
                    }
                    flood_count += 1;
                    if flood_count > 120 {
                        return Err("GenUI input rate exceeded.");
                    }
                    if envelope.generation() != generation {
                        return Err("GenUI generation mismatch.");
                    }
                    match envelope {
                        Envelope::Ack { sequence, .. } => {
                            if !sent.contains(&sequence) {
                                return Err("GenUI acknowledged an unknown snapshot.");
                            }
                            if sequence > last_ack {
                                last_ack = sequence;
                                shared.state.lock().map_err(|_| "GenUI state failed.")?.applied_sequence = sequence;
                                if pending.is_some_and(|(number, _)| number == sequence) {
                                    pending = None;
                                }
                            }
                        }
                        Envelope::Action { sequence, request, source, name, .. } => {
                            // Reserve one slot for Disconnected, even when the
                            // native foreground is not draining its queue.
                            if events.len() >= EVENT_CAPACITY - 1 {
                                return Err("GenUI event queue overflow.");
                            }
                            let action = {
                                let mut state = shared.state.lock().map_err(|_| "GenUI state failed.")?;
                                if state.closed
                                    || !state.connected
                                    || sequence != state.sequence
                                    || sequence != last_sent
                                    || sequence != last_ack
                                    || request <= state.received_request
                                    || state.latest.surface.action(&source) != Some(name)
                                {
                                    return Err("Stale or unauthorized GenUI action.");
                                }
                                state.received_request = request;
                                BridgeAction {
                                    generation: generation.to_owned(),
                                    sequence,
                                    request,
                                    source,
                                    name,
                                    session_uid: state.latest.session_uid,
                                    task: state.latest.task.clone(),
                                }
                            };
                            events.try_send(BridgeEvent::Action(action))
                                .map_err(|_| "GenUI event queue overflow.")?;
                        }
                        Envelope::Error { .. } => return Err("GenUI renderer reported failure."),
                        _ => return Err("GenUI frame has invalid direction."),
                    }
                }
                _ = tick.tick() => {
                    if pending.is_some_and(|(_, deadline)| Instant::now() >= deadline) {
                        return Err("GenUI snapshot acknowledgement timed out.");
                    }
                    if pending.is_none() && Instant::now() >= next_send {
                        let update = {
                            let state = shared.state.lock().map_err(|_| "GenUI state failed.")?;
                            (state.sequence != last_sent).then(||
                                snapshot(generation, state.sequence, &state.latest))
                        };
                        if let Some(update) = update {
                            let Envelope::Snapshot { sequence, .. } = &update else {
                                unreachable!();
                            };
                            let sequence = *sequence;
                            write_frame(&mut writer, &update).await?;
                            next_send = Instant::now() + FRAME_INTERVAL;
                            last_sent = sequence;
                            sent.push_back(sequence);
                            if sent.len() > 256 {
                                sent.pop_front();
                            }
                            pending = Some((sequence, Instant::now() + ACK_TIMEOUT));
                        }
                    }
                }
            }
        }
    }

    async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Envelope, &'static str> {
        let mut length = [0_u8; 4];
        reader
            .read_exact(&mut length[..1])
            .await
            .map_err(|_| "GenUI connection closed.")?;
        timeout(FRAME_TIMEOUT, async {
            reader
                .read_exact(&mut length[1..])
                .await
                .map_err(|_| "Truncated GenUI frame header.")?;
            let length = u32::from_be_bytes(length) as usize;
            if length == 0 || length > MAX_FRAME_BYTES {
                return Err("Invalid GenUI frame length.");
            }
            let mut bytes = vec![0_u8; length];
            reader
                .read_exact(&mut bytes)
                .await
                .map_err(|_| "Truncated GenUI frame body.")?;
            Envelope::decode(&bytes)
        })
        .await
        .unwrap_or(Err("GenUI partial frame timed out."))
    }

    async fn write_frame<W: AsyncWrite + Unpin>(
        writer: &mut W,
        envelope: &Envelope,
    ) -> Result<(), &'static str> {
        let bytes = envelope.encode()?;
        let length = u32::try_from(bytes.len()).map_err(|_| "Invalid GenUI frame length.")?;
        timeout(FRAME_TIMEOUT, async {
            writer
                .write_all(&length.to_be_bytes())
                .await
                .map_err(|_| "GenUI frame write failed.")?;
            writer
                .write_all(&bytes)
                .await
                .map_err(|_| "GenUI frame write failed.")
        })
        .await
        .unwrap_or(Err("GenUI frame write timed out."))
    }

    #[cfg(test)]
    #[cfg(unix)]
    mod tests {
        include!("genui_bridge_tests.rs");
    }
}
