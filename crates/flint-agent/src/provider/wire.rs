//! Conversation messages and the request body, serialized straight from
//! borrowed history (no intermediate `serde_json::Value` tree, no copies of
//! large tool outputs).

use serde::Deserialize;
use serde::Serialize;
use serde::ser::SerializeMap;
use serde::ser::SerializeSeq;
use serde_json::Value;

use super::stream::RawToolCall;
use crate::protocol::ImageAttachment;
use crate::protocol::ReasoningEffort;

/// One message of the conversation. Serializable so sessions can be saved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Message {
    System(String),
    User(String),
    UserWithImages {
        text: String,
        images: Vec<ImageAttachment>,
    },
    /// A harness nudge: sent as a user message, but not the start of a turn.
    Nudge(String),
    /// What compaction removed: the requests and outcomes of dropped turns.
    /// Sent as a user message near the top; not the start of a turn.
    Summary(String),
    Assistant {
        content: String,
        /// Kept for DeepSeek thinking mode, which expects it back on
        /// tool-call turns within the same user turn.
        reasoning: String,
        tool_calls: Vec<RawToolCall>,
    },
    Tool {
        call_id: String,
        content: String,
    },
}

impl Message {
    /// Wire form as a `Value`. `with_reasoning` sends `reasoning_content` back.
    pub fn to_wire(&self, with_reasoning: bool) -> Value {
        serde_json::to_value(WireMessage {
            message: self,
            with_reasoning,
        })
        .unwrap_or(Value::Null)
    }
}

/// Borrowed wire view of a message.
struct WireMessage<'a> {
    message: &'a Message,
    with_reasoning: bool,
}

impl Serialize for WireMessage<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        match self.message {
            Message::System(text) => {
                map.serialize_entry("role", "system")?;
                map.serialize_entry("content", text)?;
            }
            Message::User(text) | Message::Nudge(text) | Message::Summary(text) => {
                map.serialize_entry("role", "user")?;
                map.serialize_entry("content", text)?;
            }
            Message::UserWithImages { text, images } => {
                map.serialize_entry("role", "user")?;
                let mut content = Vec::with_capacity(images.len() + 1);
                content.push(serde_json::json!({"type": "text", "text": text}));
                for image in images {
                    content.push(serde_json::json!({
                        "type": "image_url",
                        "image_url": {"url": format!("data:{};base64,{}", image.mime_type, image.data)}
                    }));
                }
                map.serialize_entry("content", &content)?;
            }
            Message::Assistant {
                content,
                reasoning,
                tool_calls,
            } => {
                map.serialize_entry("role", "assistant")?;
                map.serialize_entry("content", content)?;
                if !tool_calls.is_empty() {
                    map.serialize_entry("tool_calls", &WireToolCalls(tool_calls))?;
                }
                if self.with_reasoning && !reasoning.is_empty() {
                    map.serialize_entry("reasoning_content", reasoning)?;
                }
            }
            Message::Tool { call_id, content } => {
                map.serialize_entry("role", "tool")?;
                map.serialize_entry("tool_call_id", call_id)?;
                map.serialize_entry("content", content)?;
            }
        }
        map.end()
    }
}

struct WireToolCalls<'a>(&'a [RawToolCall]);

impl Serialize for WireToolCalls<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for call in self.0 {
            seq.serialize_element(&serde_json::json!({
                "id": call.id,
                "type": "function",
                "function": {"name": call.name, "arguments": call.arguments},
            }))?;
        }
        seq.end()
    }
}

/// Everything that varies per request.
pub struct ChatRequest<'a> {
    pub messages: &'a [Message],
    /// Assistant tool-call messages at or after this index get their
    /// reasoning replayed (the current user turn).
    pub replay_reasoning_from: usize,
    /// Tool specs, serialized once per session.
    pub tools_json: &'a str,
    pub effort: Option<ReasoningEffort>,
}

impl ChatRequest<'_> {
    /// The JSON body. `model` and the stream options are fixed per provider.
    pub fn body(&self, model: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.size_hint());
        out.extend_from_slice(b"{\"model\":");
        write_json(&mut out, model);
        out.extend_from_slice(b",\"stream\":true,\"stream_options\":{\"include_usage\":true}");
        if let Some(effort) = self.effort {
            out.extend_from_slice(b",\"reasoning_effort\":");
            write_json(&mut out, effort.as_str());
        }
        if !self.tools_json.is_empty() {
            out.extend_from_slice(b",\"tool_choice\":\"auto\",\"tools\":");
            out.extend_from_slice(self.tools_json.as_bytes());
        }
        out.extend_from_slice(b",\"messages\":[");
        for (i, message) in self.messages.iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            let with_reasoning = i >= self.replay_reasoning_from
                && matches!(message, Message::Assistant { tool_calls, .. } if !tool_calls.is_empty());
            write_json(
                &mut out,
                &WireMessage {
                    message,
                    with_reasoning,
                },
            );
        }
        out.extend_from_slice(b"]}");
        out
    }

    fn size_hint(&self) -> usize {
        self.tools_json.len()
            + 256
            + self
                .messages
                .iter()
                .map(|m| match m {
                    Message::System(t)
                    | Message::User(t)
                    | Message::Nudge(t)
                    | Message::Summary(t) => t.len(),
                    Message::UserWithImages { text, images } => {
                        text.len() + images.iter().map(|i| i.data.len() + 100).sum::<usize>()
                    }
                    Message::Assistant {
                        content,
                        reasoning,
                        tool_calls,
                    } => content.len() + reasoning.len() + tool_calls.iter().map(|c| c.arguments.len() + 96).sum::<usize>(),
                    Message::Tool { content, .. } => content.len(),
                } + 48)
                .sum::<usize>()
    }
}

fn write_json<T: Serialize + ?Sized>(out: &mut Vec<u8>, value: &T) {
    // Serializing strings and maps into a Vec cannot fail.
    let _ = serde_json::to_writer(&mut *out, value);
}

#[cfg(test)]
mod image_tests {
    use super::*;

    #[test]
    fn serializes_user_images_as_multimodal_content() {
        let image = ImageAttachment {
            name: "shot.png".into(),
            mime_type: "image/png".into(),
            data: "aGVsbG8=".into(),
        };
        let message = Message::UserWithImages {
            text: "What is this?".into(),
            images: vec![image.clone()],
        };
        assert_eq!(
            message.to_wire(false),
            serde_json::json!({
                "role": "user",
                "content": [
                    {"type": "text", "text": "What is this?"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,aGVsbG8="}}
                ]
            })
        );
        let body = ChatRequest {
            messages: &[message],
            replay_reasoning_from: 0,
            tools_json: "",
            effort: None,
        }
        .body("model");
        let request: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            request["messages"][0]["content"][1]["image_url"]["url"],
            "data:image/png;base64,aGVsbG8="
        );
        let restored: Message = serde_json::from_str(
            &serde_json::to_string(&Message::UserWithImages {
                text: "look".into(),
                images: vec![image],
            })
            .unwrap(),
        )
        .unwrap();
        assert!(matches!(restored, Message::UserWithImages { images, .. } if images.len() == 1));
    }
}
