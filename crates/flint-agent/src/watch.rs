//! Design-contract declarations only; execution is unsupported in B1.
use crate::orchestration::*;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WatchId(String);
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NotificationId(String);
impl WatchId {
    pub fn fresh() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
    pub fn parse(value: &str) -> Result<Self, EngineError> {
        CommandId::parse(value).map(|id| Self(id.as_str().to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl NotificationId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PrProvider {
    GitHub,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PrIdentity {
    pub provider: PrProvider,
    pub origin: String, // https API origin, configured allowlist.
    pub owner: String,
    pub repository: String,
    pub number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PrWatchSpec {
    pub id: WatchId,
    pub conversation: ConversationId,
    pub pr: PrIdentity,
    pub auth_profile: Option<ProfileRef>,
    pub interval_secs: u64, // Minimum 60 seconds.
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PrState {
    Open,
    Closed,
    Merged,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PrSnapshot {
    pub state: PrState,
    pub draft: bool,
    pub head_sha: String,
    pub base_sha: String,
    pub mergeability: Option<String>,
    pub checks: std::collections::BTreeMap<String, String>,
    pub reviews: std::collections::BTreeMap<String, String>,
    pub fingerprint: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PrWatchRecord {
    pub spec: PrWatchSpec,
    pub revision: u64,
    pub paused: bool,
    pub latest: Option<PrSnapshot>,
    pub etag: Option<String>,
    pub last_checked_unix_ms: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PrNotification {
    pub id: NotificationId,
    pub watch_id: WatchId,
    pub conversation: ConversationId,
    pub previous: Option<Digest>,
    pub current: Digest,
    pub summary: String,
    pub observed_unix_ms: u64,
    pub acknowledged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WatchCommand {
    Put {
        spec: PrWatchSpec,
        expected_revision: Option<u64>,
    },
    Pause {
        id: WatchId,
        expected_revision: u64,
        paused: bool,
    },
    Remove {
        id: WatchId,
        expected_revision: u64,
    },
    Refresh {
        id: WatchId,
        expected_revision: u64,
    },
    Acknowledge {
        id: NotificationId,
        expected_fingerprint: Digest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PrPollResult {
    NotModified,
    Snapshot {
        snapshot: PrSnapshot,
        etag: Option<String>,
    },
}

pub trait PrWatchSource: Send + Sync {
    fn poll(&self, watch: PrWatchRecord) -> BoxFuture<Result<PrPollResult, EngineError>>;
}
