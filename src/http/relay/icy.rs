//! Bounded parser for interleaved ICY metadata.

use axum::body::Bytes;
use reqwest::Response;
use std::time::Duration;

const READ_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_ICY_BLOCK: usize = 4080;

/// Small cursor that preserves chunk boundaries without buffering the whole stream.
pub(super) struct Reader {
    response: Response,
    chunk: Bytes,
    offset: usize,
}

impl Reader {
    /// Starts reading a response body with one retained transport chunk.
    pub(super) fn new(response: Response) -> Self {
        Self {
            response,
            chunk: Bytes::new(),
            offset: 0,
        }
    }

    /// Returns at most `limit` bytes, preserving any remainder for the next call.
    pub(super) async fn take(&mut self, limit: usize) -> Result<Option<Bytes>, ()> {
        while self.offset == self.chunk.len() {
            self.chunk = tokio::time::timeout(READ_TIMEOUT, self.response.chunk())
                .await
                .map_err(|_| ())?
                .map_err(|_| ())?
                .unwrap_or_default();
            self.offset = 0;
            if self.chunk.is_empty() {
                return Ok(None);
            }
        }
        let end = (self.offset + limit).min(self.chunk.len());
        let bytes = self.chunk.slice(self.offset..end);
        self.offset = end;
        Ok(Some(bytes))
    }

    /// Reads one bounded ICY field even when it crosses transport chunks.
    pub(super) async fn exact(&mut self, len: usize) -> Result<Option<Vec<u8>>, ()> {
        let mut result = Vec::with_capacity(len.min(MAX_ICY_BLOCK));
        while result.len() < len {
            let Some(bytes) = self.take(len - result.len()).await? else {
                return Ok(None);
            };
            result.extend_from_slice(&bytes);
        }
        Ok(Some(result))
    }
}

/// Returns a bounded raw StreamTitle when the ICY block has valid framing and text.
pub(super) fn raw_title(block: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(block).ok()?.trim_end_matches('\0');
    let title = text.strip_prefix("StreamTitle='")?.split_once("';")?.0;
    if title.is_empty() || title.len() > 512 || title.chars().any(char::is_control) {
        None
    } else {
        Some(title.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_title_is_absent() {
        assert_eq!(raw_title(b"StreamTitle='Track';\0"), Some("Track".into()));
        assert_eq!(raw_title(b"broken"), None);
        assert_eq!(raw_title(&[255, 0]), None);
    }
}
