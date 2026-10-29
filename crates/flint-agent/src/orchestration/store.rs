use super::fs::{Directory, WriterLease};
use super::*;
use crate::{TaskOutcome, TaskRecord};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_COMMAND_BYTES: usize = 48 * 1024 * 1024;
const MAX_NODE_BYTES: usize = 64 * 1024;
const DIGEST_VERSION: u32 = 1;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Pending {
    pub receipt: CommandReceipt,
    pub object: String,
    pub acceptance: String,
    pub bytes: usize,
    pub preview: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct State {
    pub version: u32,
    pub root: SessionId,
    pub target: ProviderTarget,
    pub revision: u64,
    pub generation: u64,
    pub next_sequence: u64,
    pub receipt_head: Option<String>,
    pub receipt_count: u64,
    pub pending: Vec<Pending>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    version: u32,
    command: EngineCommand,
}
#[derive(Serialize, Deserialize)]
struct Node {
    version: u32,
    receipt: CommandReceipt,
    command_object: String,
    previous: Option<String>,
    ordinal: u64,
}

pub(crate) struct Store {
    pub lease: WriterLease,
    objects: Directory,
    pub document: Value,
    pub state: State,
    pub limits: EngineLimits,
    poisoned: bool,
    #[cfg(test)]
    pub fault: Option<&'static str>,
}

fn corrupt() -> EngineError {
    EngineError::new(
        EngineErrorCode::CorruptState,
        "Invalid saved orchestration. Existing history has been kept.",
    )
}
pub(crate) fn storage(_: std::io::Error) -> EngineError {
    EngineError::new(
        EngineErrorCode::Storage,
        "Couldn't commit private engine storage. Existing history has been kept.",
    )
}
pub(crate) fn digest(bytes: &[u8]) -> Digest {
    Digest(Sha256::digest(bytes).into())
}
fn object_name(bytes: &[u8]) -> String {
    digest_name(&digest(bytes))
}
fn digest_name(digest: &Digest) -> String {
    digest.to_hex()
}
fn valid_object(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
pub(crate) fn canonical_command(command: &EngineCommand) -> Result<(Vec<u8>, Digest), EngineError> {
    validate_intake(command)?;
    let mut value = serde_json::to_value(Envelope {
        version: DIGEST_VERSION,
        command: command.clone(),
    })
    .map_err(|_| corrupt())?;
    // Keep version-1 bytes stable even when dependency feature unification
    // enables preserve_order, including objects nested inside arrays.
    value.sort_all_objects();
    let bytes = serde_json::to_vec(&value).map_err(|_| corrupt())?;
    if bytes.len() > MAX_COMMAND_BYTES {
        return Err(EngineError::new(
            EngineErrorCode::QueueFull,
            "Command snapshot exceeds its byte limit.",
        ));
    }
    let digest = digest(&bytes);
    Ok((bytes, digest))
}

pub(crate) fn validate_intake(command: &EngineCommand) -> Result<(), EngineError> {
    let CommandPayload::Send {
        prompt,
        delivery: Delivery::NextTurn,
    } = &command.payload
    else {
        return Err(EngineError::unsupported());
    };
    validate_prompt(prompt)
}

pub(crate) fn read_document(directory: &Directory) -> Result<Option<Value>, EngineError> {
    let Some(bytes) = directory
        .read_optional("history.json", MAX_SNAPSHOT_BYTES)
        .map_err(storage)?
    else {
        return Ok(None);
    };
    let document: Value = serde_json::from_slice(&bytes).map_err(|_| corrupt())?;
    let version = document["version"].as_u64().ok_or_else(corrupt)?;
    if !matches!(version, 1..=3) {
        return Err(EngineError::new(
            EngineErrorCode::CorruptState,
            "Unsupported history version. Existing history has been kept.",
        ));
    }
    if document["turn_id"].as_u64().is_none() || !document["messages"].is_array() {
        return Err(corrupt());
    }
    // Validate legacy history/task without repairing or rewriting its source.
    serde_json::from_value::<Vec<crate::provider::Message>>(document["messages"].clone())
        .map_err(|_| corrupt())?;
    if let Some(task) = document.get("task").filter(|task| !task.is_null()) {
        serde_json::from_value::<TaskRecord>(task.clone())
            .map_err(|_| corrupt())?
            .validate()
            .map_err(|_| corrupt())?;
    }
    if version == 3 {
        validate_lineage(
            directory,
            &serde_json::from_value(document["orchestration"].clone()).map_err(|_| corrupt())?,
        )?;
    }
    Ok(Some(document))
}

pub(crate) fn validate_state(state: &State) -> Result<(), EngineError> {
    if state.version != 1
        || state.revision == 0
        || state.next_sequence == 0
        || state.pending.len() > 20
        || state.pending.len() as u64 > state.receipt_count
        || state.receipt_count >= state.next_sequence
        || state
            .receipt_head
            .as_ref()
            .is_some_and(|name| !valid_object(name))
        || (state.receipt_head.is_none() != (state.receipt_count == 0))
    {
        return Err(corrupt());
    }
    let mut bytes = 0usize;
    for (index, entry) in state.pending.iter().enumerate() {
        bytes = bytes.checked_add(entry.bytes).ok_or_else(corrupt)?;
        if bytes > 32 * 1024 * 1024
            || !valid_object(&entry.object)
            || entry.receipt.conversation.root != state.root
            || entry.receipt.conversation.child.is_some()
            || entry.receipt.accepted_sequence == 0
            || entry.receipt.accepted_sequence >= state.next_sequence
            || !valid_object(&entry.acceptance)
            || entry.preview.len() > 512
            || entry.receipt.accepted_revision == 0
            || entry.receipt.accepted_revision > state.revision
            || entry.receipt.status_revision == 0
            || entry.receipt.status_revision > state.revision
            || entry.receipt.status_revision < entry.receipt.accepted_revision
            || state.pending[..index]
                .iter()
                .any(|other| other.receipt.id == entry.receipt.id)
        {
            return Err(corrupt());
        }
    }
    Ok(())
}

fn read_object(objects: &Directory, name: &str, limit: usize) -> Result<Vec<u8>, EngineError> {
    if !valid_object(name) {
        return Err(corrupt());
    }
    let bytes = objects.read(name, limit).map_err(|_| corrupt())?;
    if object_name(&bytes) != name {
        return Err(corrupt());
    }
    Ok(bytes)
}

fn read_node(
    objects: &Directory,
    state: &State,
    name: &str,
    ordinal: u64,
) -> Result<Node, EngineError> {
    let node: Node = serde_json::from_slice(&read_object(objects, name, MAX_NODE_BYTES)?)
        .map_err(|_| corrupt())?;
    if node.version != 1
        || node.ordinal != ordinal
        || ordinal == 0
        || !valid_object(&node.command_object)
        || node.command_object != digest_name(&node.receipt.payload_digest)
        || node
            .previous
            .as_ref()
            .is_some_and(|name| !valid_object(name))
        || (node.previous.is_none() != (ordinal == 1))
        || node.receipt.conversation.root != state.root
    {
        return Err(corrupt());
    }
    Ok(node)
}

/// Read-only preflight shared by recovery, stopped v3 task mutation and
/// projections. Scan O(history nodes * <=20 active entries), one bounded node
/// at a time. No historical payload reads, unbounded receipt map or writes.
pub(crate) fn validate_lineage(directory: &Directory, state: &State) -> Result<(), EngineError> {
    validate_state(state)?;
    if state.receipt_head.is_none() {
        return Ok(());
    }
    let objects = directory
        .open_child("engine-objects")
        .map_err(|_| corrupt())?;
    // Sized by the entries actually indexed below, so this cannot silently
    // overflow if the queued-command limit is ever raised.
    let mut matched = vec![false; state.pending.len()];
    let mut next = state.receipt_head.clone();
    let mut ordinal = state.receipt_count;
    let mut before_sequence = state.next_sequence;
    while let Some(name) = next {
        let node = read_node(&objects, state, &name, ordinal)?;
        if node.receipt.accepted_sequence == 0
            || node.receipt.accepted_sequence >= before_sequence
            || node.receipt.accepted_revision == 0
            || node.receipt.accepted_revision > state.revision
        {
            return Err(corrupt());
        }
        for (index, entry) in state.pending.iter().enumerate() {
            if name == entry.acceptance {
                if node.command_object != entry.object
                    || node.receipt.id != entry.receipt.id
                    || node.receipt.conversation != entry.receipt.conversation
                    || node.receipt.payload_digest != entry.receipt.payload_digest
                    || node.receipt.accepted_sequence != entry.receipt.accepted_sequence
                    || node.receipt.accepted_revision != entry.receipt.accepted_revision
                {
                    return Err(corrupt());
                }
                matched[index] = true;
            }
        }
        before_sequence = node.receipt.accepted_sequence;
        next = node.previous;
        ordinal -= 1;
    }
    if ordinal != 0 || matched.iter().any(|found| !found) {
        return Err(corrupt());
    }
    Ok(())
}

/// Counts serialized bytes without allocating a buffer.
struct ByteCounter(usize);
impl std::io::Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Store {
    fn healthy(&self) -> Result<(), EngineError> {
        if self.poisoned {
            return Err(EngineError::new(
                EngineErrorCode::Storage,
                "Canonical commit is uncertain. Shut down and reopen before querying or accepting work.",
            ));
        }
        Ok(())
    }
    pub fn open(path: &Path, options: OrchestrationOptions) -> Result<Self, EngineError> {
        validate_limits(&options.limits)?;
        validate_target(&options.target)?;
        let lease = WriterLease::acquire(path).map_err(|error| {
            let code = if error.to_string().contains("Another writer") {
                EngineErrorCode::Busy
            } else {
                EngineErrorCode::Storage
            };
            EngineError::new(
                code,
                "Cannot acquire the exclusive engine writer. Existing history has been kept.",
            )
        })?;
        let document = read_document(&lease.directory)?.unwrap_or_else(|| {
            json!({
                "version": 2, "turn_id": 0, "messages": []
            })
        });
        let existing = document["version"] == 3;
        let mut state: State = if existing {
            serde_json::from_value(document["orchestration"].clone()).map_err(|_| corrupt())?
        } else {
            State {
                version: 1,
                root: options.root_id.clone(),
                target: options.target.clone(),
                revision: 1,
                generation: 0,
                next_sequence: 1,
                receipt_head: None,
                receipt_count: 0,
                pending: Vec::new(),
                extra: BTreeMap::new(),
            }
        };
        if state.root != options.root_id || state.target != options.target {
            return Err(EngineError::new(
                EngineErrorCode::IdentityConflict,
                "Saved session/provider identity does not match.",
            ));
        }
        if state.pending.len() > options.limits.queued_commands
            || state.pending.iter().map(|entry| entry.bytes).sum::<usize>()
                > options.limits.queued_bytes
        {
            return Err(EngineError::new(
                EngineErrorCode::QueueFull,
                "Saved queue exceeds the configured limits.",
            ));
        }
        let objects = lease.directory.child("engine-objects").map_err(storage)?;
        let mut store = Self {
            lease,
            objects,
            document,
            state: state.clone(),
            limits: options.limits,
            poisoned: false,
            #[cfg(test)]
            fault: None,
        };
        // read_document already proved membership in the canonical lineage
        // before this owner can create objects or commit recovery. Only active
        // payloads need to be opened here; historical payloads stay on disk.
        for entry in &state.pending {
            store.prompt(entry)?;
        }
        state.generation = state.generation.checked_add(1).ok_or_else(corrupt)?;
        if existing {
            state.revision = state.revision.checked_add(1).ok_or_else(corrupt)?;
        }
        for pending in &mut state.pending {
            pending.receipt.status_revision = state.revision;
            pending.receipt.state = match &pending.receipt.state {
                CommandState::Started { run_id, .. } => {
                    CommandState::Held(HoldReason::AmbiguousEffects {
                        run_id: run_id.clone(),
                        intent_ids: Vec::new(),
                    })
                }
                CommandState::Held(HoldReason::AmbiguousEffects { .. }) => {
                    pending.receipt.state.clone()
                }
                CommandState::Finished { .. } => pending.receipt.state.clone(),
                _ => CommandState::Held(HoldReason::Restart {
                    generation: state.generation,
                }),
            };
        }
        store.commit(state)?;
        Ok(store)
    }

    fn object(&self, name: &str, limit: usize) -> Result<Vec<u8>, EngineError> {
        read_object(&self.objects, name, limit)
    }
    fn node(&self, name: &str, ordinal: u64) -> Result<Node, EngineError> {
        read_node(&self.objects, &self.state, name, ordinal)
    }
    fn retained(&self, id: &CommandId) -> Result<Option<CommandReceipt>, EngineError> {
        self.healthy()?;
        // Fast path: active pending entries are already in memory — skip the
        // on-disk chain walk when the caller retries an active command.
        if let Some(entry) = self
            .state
            .pending
            .iter()
            .find(|entry| &entry.receipt.id == id)
        {
            return Ok(Some(entry.receipt.clone()));
        }
        let mut next = self.state.receipt_head.clone();
        let mut ordinal = self.state.receipt_count;
        while let Some(name) = next {
            let node = self.node(&name, ordinal)?;
            if &node.receipt.id == id {
                return Ok(Some(node.receipt));
            }
            next = node.previous;
            ordinal -= 1;
        }
        Ok(None)
    }

    pub fn receipt(&self, id: &CommandId) -> Result<Option<CommandReceipt>, EngineError> {
        self.retained(id)
    }
    pub fn submit(&mut self, command: EngineCommand) -> Result<CommandReceipt, EngineError> {
        self.healthy()?;
        let retained = self.retained(&command.id)?;
        let (bytes, payload_digest) = canonical_command(&command).map_err(|error| {
            if retained.is_some() {
                EngineError::new(
                    EngineErrorCode::IdentityConflict,
                    "Accepted command identity is immutable.",
                )
            } else {
                error
            }
        })?;
        if let Some(receipt) = retained {
            if receipt.conversation != command.conversation
                || receipt.payload_digest != payload_digest
            {
                return Err(EngineError::new(
                    EngineErrorCode::IdentityConflict,
                    "Accepted command identity is immutable.",
                ));
            }
            return Ok(receipt);
        }
        self.owned(&command.conversation)?;
        let CommandPayload::Send {
            prompt,
            delivery: Delivery::NextTurn,
        } = &command.payload
        else {
            return Err(EngineError::unsupported());
        };
        validate_prompt(prompt)?;
        if self.state.pending.len() >= self.limits.queued_commands
            || bytes.len().saturating_add(
                self.state
                    .pending
                    .iter()
                    .map(|entry| entry.bytes)
                    .sum::<usize>(),
            ) > self.limits.queued_bytes
        {
            return Err(EngineError::new(
                EngineErrorCode::QueueFull,
                "Engine queue capacity reached.",
            ));
        }
        let mut state = self.state.clone();
        state.revision = state.revision.checked_add(1).ok_or_else(corrupt)?;
        let receipt = CommandReceipt {
            id: command.id.clone(),
            conversation: command.conversation.clone(),
            task: None,
            payload_digest,
            accepted_sequence: state.next_sequence,
            accepted_revision: state.revision,
            status_revision: state.revision,
            state: CommandState::Accepted,
        };
        state.next_sequence = state.next_sequence.checked_add(1).ok_or_else(corrupt)?;
        state.receipt_count = state.receipt_count.checked_add(1).ok_or_else(corrupt)?;
        let object = object_name(&bytes);
        self.fail("object")?;
        self.objects.immutable(&object, &bytes).map_err(storage)?;
        let node = Node {
            version: 1,
            receipt: receipt.clone(),
            command_object: object.clone(),
            previous: state.receipt_head.clone(),
            ordinal: state.receipt_count,
        };
        let node_bytes = serde_json::to_vec(&node).map_err(|_| corrupt())?;
        let node_name = object_name(&node_bytes);
        self.fail("receipt")?;
        self.objects
            .immutable(&node_name, &node_bytes)
            .map_err(storage)?;
        state.receipt_head = Some(node_name.clone());
        let preview: String = prompt
            .request
            .chars()
            .scan(0usize, |size, ch| {
                *size += ch.len_utf8();
                (*size <= 512).then_some(ch)
            })
            .collect();
        state.pending.push(Pending {
            receipt: receipt.clone(),
            object,
            acceptance: node_name,
            bytes: bytes.len(),
            preview,
        });
        self.commit(state)?;
        Ok(receipt)
    }

    pub fn owned(&self, conversation: &ConversationId) -> Result<(), EngineError> {
        self.healthy()?;
        if conversation.root != self.state.root || conversation.child.is_some() {
            return Err(EngineError::new(
                EngineErrorCode::NotOwned,
                "Conversation is not owned by this B1 ledger.",
            ));
        }
        Ok(())
    }
    pub fn queue(&self, conversation: &ConversationId) -> Result<QueueProjection, EngineError> {
        self.owned(conversation)?;
        let hold = self
            .state
            .pending
            .iter()
            .find_map(|entry| match &entry.receipt.state {
                CommandState::Held(hold) => Some(hold.clone()),
                _ => None,
            })
            .or(Some(HoldReason::Paused));
        Ok(QueueProjection {
            revision: self.state.revision,
            restart_generation: self.state.generation,
            paused: true,
            hold,
            items: self
                .state
                .pending
                .iter()
                .map(|entry| QueuedCommand {
                    receipt: entry.receipt.clone(),
                    preview: entry.preview.clone(),
                    captured_bytes: entry.bytes as u64,
                })
                .collect(),
        })
    }
    pub fn prompt(&self, entry: &Pending) -> Result<PromptSnapshot, EngineError> {
        self.healthy()?;
        let bytes = self.object(&entry.object, MAX_COMMAND_BYTES)?;
        let envelope: Envelope = serde_json::from_slice(&bytes).map_err(|_| corrupt())?;
        if envelope.version != DIGEST_VERSION
            || bytes.len() != entry.bytes
            || digest(&bytes) != entry.receipt.payload_digest
            || envelope.command.id != entry.receipt.id
            || envelope.command.conversation != entry.receipt.conversation
        {
            return Err(corrupt());
        }
        let CommandPayload::Send { prompt, .. } = envelope.command.payload else {
            return Err(corrupt());
        };
        validate_prompt(&prompt).map_err(|_| corrupt())?;
        Ok(prompt)
    }
    pub fn queued_prompt(&self, id: &CommandId) -> Result<PromptSnapshot, EngineError> {
        self.healthy()?;
        self.prompt(
            self.state
                .pending
                .iter()
                .find(|entry| &entry.receipt.id == id)
                .ok_or_else(|| {
                    EngineError::new(EngineErrorCode::NotFound, "Command is not queued.")
                })?,
        )
    }
    pub fn recent(&self, before: Option<u64>, limit: usize) -> Result<ReceiptPage, EngineError> {
        self.healthy()?;
        if limit == 0 || limit > self.limits.history_page_rows {
            return Err(EngineError::new(
                EngineErrorCode::InvalidCommand,
                "Receipt page limit must be 1..=100.",
            ));
        }
        let mut next = self.state.receipt_head.clone();
        let mut ordinal = self.state.receipt_count;
        let mut receipts = Vec::new();
        // Reserve the complete JSON page envelope and worst-case continuation,
        // not merely the sum of individual receipt bodies.
        let mut counter = ByteCounter(0);
        serde_json::to_writer(
            &mut counter,
            &ReceiptPage {
                receipts: Vec::new(),
                next_before_sequence: Some(u64::MAX),
            },
        )
        .map_err(|_| corrupt())?;
        let mut bytes = counter.0;
        if bytes > self.limits.history_page_bytes {
            return Err(EngineError::new(
                EngineErrorCode::InvalidCommand,
                "Receipt page envelope exceeds its byte limit.",
            ));
        }
        let mut more = false;
        while let Some(name) = next {
            let node = self.node(&name, ordinal)?;
            next = node.previous;
            ordinal -= 1;
            if before.is_some_and(|before| node.receipt.accepted_sequence >= before) {
                continue;
            }
            let receipt = self
                .state
                .pending
                .iter()
                .find(|entry| entry.receipt.id == node.receipt.id)
                .map_or(node.receipt, |entry| entry.receipt.clone());
            let mut counter = ByteCounter(0);
            serde_json::to_writer(&mut counter, &receipt).map_err(|_| corrupt())?;
            bytes += counter.0 + usize::from(!receipts.is_empty());
            if bytes > self.limits.history_page_bytes {
                if receipts.is_empty() {
                    return Err(EngineError::new(
                        EngineErrorCode::InvalidCommand,
                        "Receipt exceeds page byte limit.",
                    ));
                }
                more = true;
                break;
            }
            receipts.push(receipt);
            if receipts.len() == limit {
                break;
            }
        }
        let next_before_sequence = if next.is_some() || more {
            receipts.last().map(|r| r.accepted_sequence)
        } else {
            None
        };
        Ok(ReceiptPage {
            receipts,
            next_before_sequence,
        })
    }
    pub fn summary(
        &self,
        conversation: &ConversationId,
    ) -> Result<ConversationSummary, EngineError> {
        self.owned(conversation)?;
        summary_from(&self.document, &self.state)
    }
    pub(crate) fn commit(&mut self, state: State) -> Result<(), EngineError> {
        if self.poisoned {
            return Err(storage(std::io::Error::other(
                "Writer must reopen after failure.",
            )));
        }
        validate_state(&state)?;
        let mut document = self.document.clone();
        document["version"] = json!(3);
        document["orchestration"] = serde_json::to_value(&state).map_err(|_| corrupt())?;
        let bytes = serde_json::to_vec(&document).map_err(|_| corrupt())?;
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(EngineError::new(
                EngineErrorCode::Storage,
                "Canonical snapshot exceeds its byte limit.",
            ));
        }
        self.fail("snapshot")?;
        #[cfg(test)]
        let fault = self.fault;
        #[cfg(not(test))]
        let fault: Option<&str> = None;
        let result = self.lease.directory.replace_with(
            "history.json",
            &bytes,
            || {
                if fault == Some("before-rename") {
                    Err(std::io::Error::other("injected"))
                } else {
                    Ok(())
                }
            },
            || {
                if fault == Some("after-rename") {
                    Err(std::io::Error::other("injected"))
                } else {
                    Ok(())
                }
            },
        );
        if let Err(error) = result {
            // Rename may have succeeded. No receipt is reported and no further
            // acceptance is allowed until reopening reconciles canonical disk.
            self.poisoned = true;
            return Err(storage(error));
        }
        self.document = document;
        self.state = state;
        Ok(())
    }
    fn fail(&self, point: &str) -> Result<(), EngineError> {
        #[cfg(test)]
        if self.fault == Some(point) {
            return Err(storage(std::io::Error::other("injected")));
        }
        let _ = point;
        Ok(())
    }
}

pub(crate) fn validate_target(target: &ProviderTarget) -> Result<(), EngineError> {
    let (profile, text) = match target {
        ProviderTarget::Native { profile, model, .. } => (profile, model),
        ProviderTarget::Acp {
            profile,
            agent_id,
            options,
        } => {
            if options.len() > 32
                || options
                    .iter()
                    .any(|(key, value)| key.len() > 128 || value.len() > 512)
            {
                return Err(corrupt());
            }
            (profile, agent_id)
        }
    };
    SessionId::parse(&profile.id)?;
    if profile.revision == 0 || text.trim().is_empty() || text.len() > 512 {
        return Err(EngineError::new(
            EngineErrorCode::InvalidCommand,
            "Invalid provider target.",
        ));
    }
    Ok(())
}
fn validate_limits(limits: &EngineLimits) -> Result<(), EngineError> {
    let defaults = EngineLimits::default();
    if limits.queued_commands == 0
        || limits.queued_commands > defaults.queued_commands
        || limits.queued_bytes == 0
        || limits.queued_bytes > defaults.queued_bytes
        || limits.command_inbox == 0
        || limits.command_inbox > defaults.command_inbox
        || limits.history_page_rows == 0
        || limits.history_page_rows > defaults.history_page_rows
        || limits.history_page_bytes == 0
        || limits.history_page_bytes > defaults.history_page_bytes
        || limits.record_delivery == 0
        || limits.record_delivery > defaults.record_delivery
    {
        return Err(EngineError::new(
            EngineErrorCode::InvalidCommand,
            "Engine limits exceed B1 bounds.",
        ));
    }
    Ok(())
}
fn validate_prompt(prompt: &PromptSnapshot) -> Result<(), EngineError> {
    let bytes = prompt.images.iter().fold(
        prompt
            .request
            .len()
            .saturating_add(prompt.effective_text.len()),
        |sum, image| {
            sum.saturating_add(image.data.len())
                .saturating_add(image.name.len())
        },
    );
    if bytes > 32 * 1024 * 1024 {
        return Err(EngineError::new(
            EngineErrorCode::QueueFull,
            "Frozen prompt exceeds its input byte limit.",
        ));
    }
    if (prompt.request.trim().is_empty() && prompt.images.is_empty())
        || prompt.images.len() > 4
        || prompt.citations.len() > 32
        || prompt.images.iter().any(|image| {
            image.name.is_empty()
                || image.name.len() > 512
                || !matches!(
                    image.mime_type.as_str(),
                    "image/png" | "image/jpeg" | "image/webp"
                )
                || image.data.is_empty()
        })
        || prompt
            .citations
            .iter()
            .any(|citation| citation.utf8_start > citation.utf8_end || citation.row.ordinal == 0)
    {
        return Err(EngineError::new(
            EngineErrorCode::InvalidCommand,
            "Invalid frozen prompt snapshot.",
        ));
    }
    Ok(())
}
pub(crate) fn summary_from(
    document: &Value,
    state: &State,
) -> Result<ConversationSummary, EngineError> {
    let outcome = document
        .get("task")
        .and_then(|task| task.get("assessment"))
        .and_then(|assessment| assessment.get("outcome"))
        .map(|outcome| serde_json::from_value(outcome.clone()))
        .transpose()
        .map_err(|_| corrupt())?
        .unwrap_or(TaskOutcome::Unknown);
    Ok(ConversationSummary {
        conversation: ConversationId {
            root: state.root.clone(),
            child: None,
        },
        title: None,
        target: state.target.clone(),
        // B1 creates acceptance receipts, not engine event records.
        last_record_sequence: 0,
        snapshot_revision: state.revision,
        last_turn_id: document["turn_id"].as_u64().ok_or_else(corrupt)?,
        running: false,
        queued: state.pending.len(),
        hold: Some(HoldReason::Paused),
        outcome,
        unread: false,
        preview: state
            .pending
            .last()
            .map_or_else(String::new, |entry| entry.preview.clone()),
    })
}
