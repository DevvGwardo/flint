//! Design-contract declarations only; execution is unsupported in B1.
use crate::orchestration::*;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AssetId(String);
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AssetRef {
    pub id: AssetId,
    pub mime_type: String,
    pub bytes: u64,
    pub digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ContentRef {
    Inline(String),  // <=16 KiB, safe plain/Markdown source.
    Asset(AssetRef), // Engine-owned relative object, not arbitrary PathBuf.
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DiagramLanguage {
    Mermaid,
    Dot,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RichPart {
    Text {
        content: ContentRef,
        markdown: bool,
    },
    Image {
        asset: AssetRef,
        alt: String,
    },
    HtmlSource {
        content: ContentRef,
    },
    DiagramSource {
        language: DiagramLanguage,
        content: ContentRef,
    },
    ResourceLink {
        uri: String,
        name: String,
        mime_type: Option<String>,
    },
    Unsupported {
        mime_type: String,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RichOutput {
    pub row: RowId,
    pub call_id: Option<String>,
    pub parts: Vec<RichPart>, // <=32.
    pub model_text: String,   // Existing bounded text result + nontext placeholders.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RichLimits {
    pub parts: usize,               // 32.
    pub decoded_asset_bytes: usize, // <=1 MiB at current MCP 1 MiB frame cap.
    pub output_bytes: usize,        // <=4 MiB across ACP output parts.
    pub source_bytes: usize,        // <=256 KiB HTML/diagram.
}
