//! Validated data, never executable UI. Google's experimental renderer is not
//! the security boundary; every complete surface passes this host-owned schema.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{TaskOutcome, TaskRecord};

pub const BRIDGE_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 256 * 1024;
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub const CATALOG_ID: &str = "dev.hooter.safe.v1";
pub const SURFACE_ID: &str = "task";
const MAX_COMPONENTS: usize = 256;
const MAX_TEXT_BYTES: usize = 16 * 1024;
const MAX_DEPTH: usize = 24;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceSnapshot {
    pub surface_id: String,
    pub catalog_id: String,
    pub components: Vec<Component>,
    pub data: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "component", deny_unknown_fields)]
pub enum Component {
    PlainText {
        id: String,
        text: TextValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<TextStyle>,
    },
    Column {
        id: String,
        children: Vec<String>,
    },
    Row {
        id: String,
        children: Vec<String>,
    },
    Card {
        id: String,
        child: String,
    },
    Divider {
        id: String,
    },
    Button {
        id: String,
        child: String,
        action: Action,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TextValue {
    Literal(String),
    Binding(TextBinding),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextBinding {
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextStyle {
    Heading,
    Body,
    Muted,
    Code,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionName {
    OpenTask,
    StagePrompt,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub name: ActionName,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskMetadata {
    pub id: u64,
    pub contract_revision: u64,
    pub work_revision: u64,
    pub outcome: TaskOutcome,
}

impl TaskMetadata {
    pub fn from_task(task: &TaskRecord) -> Self {
        Self {
            id: task.contract.id,
            contract_revision: task.contract.revision,
            work_revision: task.work_revision,
            outcome: task
                .assessment
                .as_ref()
                .map_or(TaskOutcome::Unknown, |assessment| assessment.outcome),
        }
    }
    fn validate(&self) -> Result<(), &'static str> {
        if [self.id, self.contract_revision, self.work_revision]
            .iter()
            .any(|value| *value > MAX_SAFE_INTEGER)
        {
            return Err("GenUI task identifiers exceed the safe integer limit.");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    MalformedFrame,
    InvalidSnapshot,
    RenderFailed,
    Unauthorized,
}

// No flatten: serde's deny_unknown_fields must hold at every boundary.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Envelope {
    Hello {
        version: u32,
        generation: String,
        token: String,
    },
    Ready {
        version: u32,
        generation: String,
    },
    Snapshot {
        version: u32,
        generation: String,
        sequence: u64,
        task: Option<TaskMetadata>,
        surface: SurfaceSnapshot,
    },
    Ack {
        version: u32,
        generation: String,
        sequence: u64,
    },
    Action {
        version: u32,
        generation: String,
        sequence: u64,
        request: u64,
        source: String,
        name: ActionName,
    },
    Error {
        version: u32,
        generation: String,
        code: ErrorCode,
    },
    Shutdown {
        version: u32,
        generation: String,
    },
}

impl std::fmt::Debug for Envelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Frames may contain a bootstrap capability or private task text.
        f.debug_struct("GenUI envelope")
            .field("kind", &self.kind())
            .finish_non_exhaustive()
    }
}

impl Component {
    pub fn id(&self) -> &str {
        match self {
            Self::PlainText { id, .. }
            | Self::Column { id, .. }
            | Self::Row { id, .. }
            | Self::Card { id, .. }
            | Self::Divider { id }
            | Self::Button { id, .. } => id,
        }
    }
    fn children(&self) -> &[String] {
        match self {
            Self::Column { children, .. } | Self::Row { children, .. } => children,
            Self::Card { child, .. } | Self::Button { child, .. } => std::slice::from_ref(child),
            Self::PlainText { .. } | Self::Divider { .. } => &[],
        }
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

impl SurfaceSnapshot {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.surface_id != SURFACE_ID || self.catalog_id != CATALOG_ID {
            return Err("Unsupported GenUI surface or catalog.");
        }
        if self.components.is_empty()
            || self.components.len() > MAX_COMPONENTS
            || self.data.len() > 64
        {
            return Err("GenUI surface exceeds its component/data limit.");
        }
        if self
            .data
            .iter()
            .any(|(key, value)| !identifier(key) || value.len() > MAX_TEXT_BYTES)
        {
            return Err("Invalid GenUI data key or oversized text.");
        }
        let mut index = BTreeMap::new();
        let mut rendered_text_bytes = 0usize;
        for component in &self.components {
            if !identifier(component.id()) || index.insert(component.id(), component).is_some() {
                return Err("Invalid or duplicate GenUI component identifier.");
            }
            if component.children().len() > 64 {
                return Err("GenUI container exceeds its child limit.");
            }
            if let Component::PlainText { text, .. } = component {
                let text_bytes = match text {
                    TextValue::Literal(value) => value.len(),
                    TextValue::Binding(binding) => binding
                        .path
                        .strip_prefix('/')
                        .and_then(|key| self.data.get(key))
                        .map_or(0, String::len),
                };
                rendered_text_bytes = rendered_text_bytes.saturating_add(text_bytes);
                if rendered_text_bytes > 1024 * 1024 {
                    return Err("GenUI rendered text exceeds its byte limit.");
                }
                match text {
                    TextValue::Literal(value) if value.len() <= MAX_TEXT_BYTES => {}
                    TextValue::Binding(binding)
                        if binding
                            .path
                            .strip_prefix('/')
                            .is_some_and(|key| identifier(key) && self.data.contains_key(key)) => {}
                    _ => return Err("Invalid GenUI text or data binding."),
                }
            }
        }
        if !index.contains_key("root") {
            return Err("GenUI surface has no root component.");
        }
        let mut parents = HashSet::new();
        for component in &self.components {
            if component
                .children()
                .iter()
                .any(|child| !identifier(child) || !index.contains_key(child.as_str()))
            {
                return Err("GenUI surface references an unknown component.");
            }
            for child in component.children() {
                if child == "root" || !parents.insert(child.as_str()) {
                    return Err("GenUI components must form a tree without reused children.");
                }
            }
        }
        let mut visiting = HashSet::new();
        let mut heights = HashMap::new();
        graph_height("root", &index, &mut visiting, &mut heights, 1)?;
        if heights.len() != self.components.len() {
            return Err("GenUI surface contains unreachable components.");
        }
        if serde_json::to_vec(self)
            .map_err(|_| "Invalid GenUI surface.")?
            .len()
            > MAX_FRAME_BYTES
        {
            return Err("GenUI surface exceeds the byte limit.");
        }
        Ok(())
    }

    pub fn action(&self, source: &str) -> Option<ActionName> {
        self.components
            .iter()
            .find_map(|component| match component {
                Component::Button { id, action, .. } if id == source => Some(action.name),
                _ => None,
            })
    }
}

fn graph_height<'a>(
    id: &'a str,
    index: &BTreeMap<&'a str, &'a Component>,
    visiting: &mut HashSet<&'a str>,
    heights: &mut HashMap<&'a str, usize>,
    depth: usize,
) -> Result<usize, &'static str> {
    if depth > MAX_DEPTH {
        return Err("GenUI graph exceeds its depth limit.");
    }
    if let Some(height) = heights.get(id).copied() {
        if depth + height - 1 > MAX_DEPTH {
            return Err("GenUI graph exceeds its depth limit.");
        }
        return Ok(height);
    }
    if !visiting.insert(id) {
        return Err("GenUI graph contains a cycle.");
    }
    let component = index
        .get(id)
        .ok_or("GenUI graph has a missing component.")?;
    let mut height = 1;
    for child in component.children() {
        height = height.max(1 + graph_height(child, index, visiting, heights, depth + 1)?);
    }
    visiting.remove(id);
    heights.insert(id, height);
    Ok(height)
}

impl Envelope {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Hello { .. } => "hello",
            Self::Ready { .. } => "ready",
            Self::Snapshot { .. } => "snapshot",
            Self::Ack { .. } => "ack",
            Self::Action { .. } => "action",
            Self::Error { .. } => "error",
            Self::Shutdown { .. } => "shutdown",
        }
    }
    pub fn generation(&self) -> &str {
        match self {
            Self::Hello { generation, .. }
            | Self::Ready { generation, .. }
            | Self::Snapshot { generation, .. }
            | Self::Ack { generation, .. }
            | Self::Action { generation, .. }
            | Self::Error { generation, .. }
            | Self::Shutdown { generation, .. } => generation,
        }
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        let version = match self {
            Self::Hello { version, .. }
            | Self::Ready { version, .. }
            | Self::Snapshot { version, .. }
            | Self::Ack { version, .. }
            | Self::Action { version, .. }
            | Self::Error { version, .. }
            | Self::Shutdown { version, .. } => *version,
        };
        if version != BRIDGE_VERSION
            || self.generation().is_empty()
            || self.generation().len() > 64
            || !self
                .generation()
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("Invalid GenUI bridge version or generation.");
        }
        match self {
            Self::Hello { token, .. } if token.is_empty() || token.len() > 256 => {
                return Err("Invalid GenUI bootstrap capability.");
            }
            Self::Snapshot {
                sequence,
                task,
                surface,
                ..
            } => {
                positive_counter(*sequence)?;
                if let Some(task) = task {
                    task.validate()?;
                }
                surface.validate()?;
            }
            Self::Ack { sequence, .. } => positive_counter(*sequence)?,
            Self::Action {
                sequence,
                request,
                source,
                ..
            } => {
                positive_counter(*sequence)?;
                positive_counter(*request)?;
                if !identifier(source) {
                    return Err("Invalid GenUI action source.");
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
            return Err("GenUI frame exceeds its byte limit.");
        }
        json_depth(bytes)?;
        let envelope: Self = serde_json::from_slice(bytes).map_err(|_| "Malformed GenUI frame.")?;
        envelope.validate()?;
        Ok(envelope)
    }
    pub fn encode(&self) -> Result<Vec<u8>, &'static str> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| "Invalid GenUI frame.")?;
        if bytes.len() > MAX_FRAME_BYTES {
            return Err("GenUI frame exceeds its byte limit.");
        }
        Ok(bytes)
    }
}

fn positive_counter(value: u64) -> Result<(), &'static str> {
    if value == 0 || value > MAX_SAFE_INTEGER {
        Err("Invalid GenUI sequence counter.")
    } else {
        Ok(())
    }
}

fn json_depth(bytes: &[u8]) -> Result<(), &'static str> {
    let mut depth = 0_u32;
    let mut quoted = false;
    let mut escaped = false;
    for byte in bytes {
        if quoted {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > 32 {
                        return Err("GenUI JSON exceeds its nesting limit.");
                    }
                }
                b'}' | b']' => {
                    depth = depth.checked_sub(1).ok_or("Malformed GenUI frame.")?;
                }
                _ => {}
            }
        }
    }
    if depth != 0 || quoted {
        return Err("Malformed GenUI frame.");
    }
    Ok(())
}

#[cfg(test)]
#[path = "genui_tests.rs"]
mod tests;
