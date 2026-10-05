//! Bounded UTF-8 tails and incremental decoding for independent byte pipes.

/// Retain the newest complete characters within a hard byte budget.
pub(crate) fn append_tail(tail: &mut String, text: &str, limit: usize) -> bool {
    let truncated = tail.len().saturating_add(text.len()) > limit;
    if text.len() >= limit {
        let mut start = text.len() - limit;
        while !text.is_char_boundary(start) {
            start += 1;
        }
        tail.clear();
        tail.push_str(&text[start..]);
    } else {
        let mut cut = tail.len().saturating_add(text.len()).saturating_sub(limit);
        while !tail.is_char_boundary(cut) {
            cut += 1;
        }
        tail.drain(..cut);
        tail.push_str(text);
    }
    truncated
}

#[derive(Default)]
pub(crate) struct Utf8Decoder {
    pending: Vec<u8>,
}

impl Utf8Decoder {
    /// Invalid sequences are replaced; incomplete sequences wait for the next
    /// read. At EOF an incomplete tail becomes one replacement character.
    pub(crate) fn decode(&mut self, bytes: &[u8], eof: bool) -> String {
        self.pending.extend_from_slice(bytes);
        let mut text = String::new();
        let mut offset = 0;
        while offset < self.pending.len() {
            match std::str::from_utf8(&self.pending[offset..]) {
                Ok(valid) => {
                    text.push_str(valid);
                    offset = self.pending.len();
                }
                Err(error) => {
                    let end = offset + error.valid_up_to();
                    text.push_str(
                        std::str::from_utf8(&self.pending[offset..end]).expect("valid prefix"),
                    );
                    offset = end;
                    match error.error_len() {
                        Some(len) => {
                            text.push('\u{fffd}');
                            offset += len;
                        }
                        None if eof => {
                            text.push('\u{fffd}');
                            offset = self.pending.len();
                        }
                        None => break,
                    }
                }
            }
        }
        self.pending.drain(..offset);
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_split_matches_whole_stream_including_invalid_eof() {
        let bytes = "界é🙂"
            .as_bytes()
            .iter()
            .copied()
            .chain([0xff, 0xf0, 0x9f])
            .collect::<Vec<_>>();
        for split in 0..=bytes.len() {
            let mut decoder = Utf8Decoder::default();
            let text = decoder.decode(&bytes[..split], false)
                + &decoder.decode(&bytes[split..], false)
                + &decoder.decode(&[], true);
            assert_eq!(text, String::from_utf8_lossy(&bytes));
            assert!(decoder.pending.is_empty());
        }
    }

    #[test]
    fn tails_never_exceed_byte_budget_or_split_unicode() {
        let mut tail = String::new();
        for limit in 0..8 {
            tail.clear();
            append_tail(&mut tail, "界é🙂", limit);
            assert!(tail.len() <= limit);
            append_tail(&mut tail, "界", limit);
            assert!(tail.len() <= limit);
        }
    }
}
