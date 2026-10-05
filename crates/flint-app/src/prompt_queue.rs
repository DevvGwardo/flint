//! Bounded, session-owned prompt snapshots. Attachments are captured before
//! entering the queue, so later file changes cannot change a queued request.

use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::path::Path;

use flint_agent::ImageAttachment;
use serde::{Deserialize, Serialize};

pub const MAX_PROMPTS: usize = 20;
pub const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_SAVED_BYTES: u64 = 48 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prompt {
    pub id: u64,
    /// Editable text, without the image labels shown in the transcript.
    pub text: String,
    pub shown: String,
    /// Text and frozen file context sent to the agent.
    pub message: String,
    pub images: Vec<ImageAttachment>,
}

impl Prompt {
    pub fn new(text: String, message: String, images: Vec<ImageAttachment>) -> Self {
        let shown = images.iter().fold(text.clone(), |mut shown, image| {
            if !shown.is_empty() {
                shown.push('\n');
            }
            shown.push_str(&format!("[Image: {}]", image.name));
            shown
        });
        Self {
            id: 0,
            text,
            shown,
            message,
            images,
        }
    }

    pub fn bytes(&self) -> usize {
        self.text.len()
            + self.shown.len()
            + self.message.len()
            + self
                .images
                .iter()
                .map(|image| image.name.len() + image.mime_type.len() + image.data.len())
                .sum::<usize>()
    }

    /// Editing changes the instruction, not the already captured context.
    pub fn with_text(&self, text: String) -> Self {
        let message = match self.message.strip_prefix(&self.text) {
            Some(context) => format!("{text}{context}"),
            None => format!("{text}\n\n[Captured context]\n{}", self.message),
        };
        let mut prompt = Self::new(text, message, self.images.clone());
        prompt.id = self.id;
        prompt
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Queue {
    pub items: VecDeque<Prompt>,
    pub paused: bool,
    next_id: u64,
    #[serde(skip)]
    pub error: Option<String>,
    /// ACP steering interrupts the current turn, then dispatches this entry.
    #[serde(skip)]
    pub steer_after_turn: Option<u64>,
    #[serde(skip)]
    pub unreadable: bool,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    version: u32,
    queue: Queue,
}

#[derive(Serialize)]
struct SavedRef<'a> {
    version: u32,
    queue: &'a Queue,
}

struct BoundedWriter<W> {
    inner: W,
    remaining: u64,
}

impl<W: std::io::Write> std::io::Write for BoundedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(std::io::Error::other(
                "Saved prompt queue exceeds its size limit.",
            ));
        }
        let written = self.inner.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl Queue {
    pub fn enqueue(&mut self, mut prompt: Prompt) -> Result<u64, String> {
        if self.unreadable {
            return Err("The saved queue needs repair. Its file has been kept; move or repair it before adding prompts.".into());
        }
        if self.items.len() >= MAX_PROMPTS {
            return Err(format!(
                "The queue holds up to {MAX_PROMPTS} prompts. Remove one before adding another."
            ));
        }
        if prompt
            .bytes()
            .saturating_add(self.items.iter().map(Prompt::bytes).sum())
            > MAX_BYTES
        {
            return Err(
                "The queue's attachment limit is 32 MB. Remove a prompt or image first.".into(),
            );
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("Queue identifier limit reached.")?;
        prompt.id = self.next_id;
        let id = prompt.id;
        self.items.push_back(prompt);
        self.error = None;
        Ok(id)
    }

    pub fn position(&self, id: u64) -> Option<usize> {
        self.items.iter().position(|prompt| prompt.id == id)
    }

    pub fn remove(&mut self, id: u64) -> Option<Prompt> {
        self.items.remove(self.position(id)?)
    }

    pub fn replace(&mut self, id: u64, prompt: Prompt) -> Result<(), String> {
        let ix = self
            .position(id)
            .ok_or("This prompt is no longer queued.")?;
        let size = self
            .items
            .iter()
            .enumerate()
            .filter(|(n, _)| *n != ix)
            .map(|(_, item)| item.bytes())
            .sum::<usize>();
        if size.saturating_add(prompt.bytes()) > MAX_BYTES {
            return Err("The edited prompt exceeds the queue's 32 MB limit.".into());
        }
        self.items[ix] = prompt;
        Ok(())
    }

    pub fn move_by(&mut self, id: u64, delta: isize) {
        if let Some(ix) = self.position(id) {
            let target = ix.saturating_add_signed(delta).min(self.items.len() - 1);
            self.items.swap(ix, target);
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        if self.unreadable {
            return Err(std::io::Error::other(
                "The unreadable queue file has been kept.",
            ));
        }
        std::fs::create_dir_all(dir)?;
        let mut file = tempfile::NamedTempFile::new_in(dir)?;
        {
            let mut writer = BoundedWriter {
                inner: std::io::BufWriter::new(file.as_file_mut()),
                remaining: MAX_SAVED_BYTES,
            };
            serde_json::to_writer(
                &mut writer,
                &SavedRef {
                    version: 1,
                    queue: self,
                },
            )?;
            writer.flush()?;
        }
        file.persist(dir.join("prompt-queue.json"))
            .map_err(|error| error.error)?;
        Ok(())
    }

    pub fn load(dir: &Path) -> Result<Self, String> {
        let file = match std::fs::File::open(dir.join("prompt-queue.json")) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(_) => {
                return Err("Couldn't read the saved prompt queue. Its file has been kept.".into());
            }
        };
        let mut bytes = Vec::new();
        file.take(MAX_SAVED_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Couldn't read the saved prompt queue.")?;
        if bytes.len() as u64 > MAX_SAVED_BYTES {
            return Err("The saved prompt queue is too large to load.".into());
        }
        let saved: Saved = serde_json::from_slice(&bytes)
            .map_err(|_| "Couldn't parse the saved prompt queue. Its file has been kept.")?;
        let mut queue = saved.queue;
        if saved.version != 1
            || queue.items.len() > MAX_PROMPTS
            || queue.items.iter().map(Prompt::bytes).sum::<usize>() > MAX_BYTES
            || queue.items.iter().any(|prompt| {
                prompt.id == 0
                    || prompt.id > queue.next_id
                    || prompt.images.len() > crate::image_attach::MAX_IMAGES
            })
            || queue.items.iter().enumerate().any(|(ix, prompt)| {
                queue
                    .items
                    .iter()
                    .take(ix)
                    .any(|other| other.id == prompt.id)
            })
        {
            return Err("The saved prompt queue is invalid. Its file has been kept.".into());
        }
        // Never run pending user work automatically after a restart.
        queue.paused = !queue.items.is_empty();
        Ok(queue)
    }
}

#[cfg(test)]
#[path = "prompt_queue_tests.rs"]
mod tests;
