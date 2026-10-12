//! Private-channel contract only. No secret sinks are enabled in B1.
use crate::orchestration::{ConversationId, ProfileRef, RunId};
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretPurpose {
    NativeProviderAuth { profile: ProfileRef },
    ReadOnlyPrAuth { profile: ProfileRef, origin: String },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretDescriptor {
    pub conversation: ConversationId,
    pub run_id: RunId,
    pub purpose: SecretPurpose,
    pub title: String,
    pub expires_after_ms: u64,
}
pub struct SecretBytes {
    _private: (),
}
impl SecretBytes {
    pub fn new(mut bytes: Vec<u8>) -> Result<Self, SecretError> {
        // No value is retained or exposed by this unfinished B1 surface.
        for byte in &mut bytes {
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        Err(SecretError::Unsupported)
    }
    pub fn clear(&mut self) {}
}
impl std::fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}
pub struct SecretRequest {
    pub descriptor: SecretDescriptor,
    pub reply: SecretReplyHandle,
}
pub struct SecretReplyHandle {
    _private: (),
}
pub struct SecretInbox {
    pub(crate) _private: (),
}
impl SecretInbox {
    pub async fn recv(&self) -> Result<SecretRequest, SecretError> {
        Err(SecretError::Unsupported)
    }
}
impl SecretReplyHandle {
    pub fn submit(self, _value: SecretBytes) -> Result<(), SecretError> {
        Err(SecretError::Unsupported)
    }
    pub fn cancel(self) {}
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretError {
    Expired,
    Cancelled,
    WrongOwner,
    TooLarge,
    Empty,
    Closed,
    Unsupported,
}
