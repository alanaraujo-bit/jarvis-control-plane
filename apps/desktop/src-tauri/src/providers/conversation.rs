//! The normalised conversation model (§24).
//!
//! Conversation View is a *structured representation of development*, not a
//! chat transcript. Providers therefore normalise into these items rather than
//! into "messages", so the UI can render a tool call, a diff and an approval
//! as themselves instead of as paragraphs of text.
//!
//! Normalising here also keeps provider quirks out of the UI: whatever shape
//! Claude Code or Codex writes, a component only ever sees a `ConversationItem`.

use serde::{Deserialize, Serialize};

use crate::session::event::Confidence;

/// Who produced an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    User,
    Assistant,
    System,
}

/// Token usage for one turn, always carrying its confidence (§28).
///
/// The confidence is set by the adapter that produced the numbers, because
/// that is the only place which knows where they came from. Deciding it later —
/// at the point of storage, say — would mean inventing it, and everything would
/// end up labelled Official including figures that were merely derived.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
    pub cost_usd: Option<f64>,
    pub model: Option<String>,
    pub confidence: Confidence,
    /// Percentage of the account quota consumed, when the provider reports it.
    pub limit_percent: Option<f64>,
    /// When that quota window resets, in epoch milliseconds.
    pub limit_resets_at: Option<i64>,
}

impl Default for TokenUsage {
    fn default() -> Self {
        Self {
            input: None,
            output: None,
            cache_read: None,
            cache_write: None,
            cost_usd: None,
            model: None,
            // Nothing is known about provenance until an adapter says so.
            confidence: Confidence::Unknown,
            limit_percent: None,
            limit_resets_at: None,
        }
    }
}

impl TokenUsage {
    /// Whether this sample carries any figure worth recording.
    pub fn is_empty(&self) -> bool {
        self.input.is_none()
            && self.output.is_none()
            && self.cache_read.is_none()
            && self.cache_write.is_none()
            && self.cost_usd.is_none()
            && self.limit_percent.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConversationItem {
    #[serde(rename_all = "camelCase")]
    Message {
        role: Role,
        text: String,
        ts_ms: i64,
        /// Set for assistant turns that reported usage.
        usage: Option<TokenUsage>,
    },
    /// The agent's own reasoning, kept separate so it can be collapsed.
    #[serde(rename_all = "camelCase")]
    Thinking { text: String, ts_ms: i64 },
    #[serde(rename_all = "camelCase")]
    ToolCall {
        id: String,
        name: String,
        /// A short, human-readable rendering of the important input.
        summary: String,
        ts_ms: i64,
    },
    #[serde(rename_all = "camelCase")]
    ToolResult {
        id: String,
        ok: bool,
        summary: String,
        ts_ms: i64,
    },
    #[serde(rename_all = "camelCase")]
    Error { message: String, ts_ms: i64 },
    /// A file the session touched. Providers report this; discarding it would
    /// throw away the answer to "what did this agent actually change?" (§39).
    #[serde(rename_all = "camelCase")]
    FileChange { path: String, ts_ms: i64 },
}

impl ConversationItem {
    pub fn ts_ms(&self) -> i64 {
        match self {
            Self::Message { ts_ms, .. }
            | Self::Thinking { ts_ms, .. }
            | Self::ToolCall { ts_ms, .. }
            | Self::ToolResult { ts_ms, .. }
            | Self::FileChange { ts_ms, .. }
            | Self::Error { ts_ms, .. } => *ts_ms,
        }
    }
}

/// Parse an RFC3339 timestamp into epoch milliseconds.
///
/// Deliberately narrow: provider transcripts emit a fixed
/// `YYYY-MM-DDTHH:MM:SS.sssZ` shape, and a full date-time parser would be a
/// dependency bought for one format.
pub fn parse_timestamp(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let num = |from: usize, to: usize| text.get(from..to)?.parse::<i64>().ok();

    let year = num(0, 4)?;
    let month = num(5, 7)?;
    let day = num(8, 10)?;
    let hour = num(11, 13)?;
    let minute = num(14, 16)?;
    let second = num(17, 19)?;

    // Fractional seconds are optional and may be any length.
    let millis = text
        .split_once('.')
        .and_then(|(_, rest)| {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() {
                return None;
            }
            let padded = format!("{digits:0<3}");
            padded.get(..3)?.parse::<i64>().ok()
        })
        .unwrap_or(0);

    let days = days_from_civil(year, month as u32, day as u32);
    Some(((days * 86_400 + hour * 3_600 + minute * 60 + second) * 1_000) + millis)
}

/// Howard Hinnant's `days_from_civil`.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

/// Shorten text for a one-line summary without cutting a character in half.
pub fn truncate(text: &str, max_chars: usize) -> String {
    let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.chars().count() <= max_chars {
        return cleaned;
    }
    let kept: String = cleaned.chars().take(max_chars).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_timestamps() {
        // Verbatim from a real Claude Code transcript. The expected value is
        // the epoch-millisecond reading of that instant.
        let ms = parse_timestamp("2026-08-22T21:57:01.375Z").unwrap();
        assert_eq!(ms, 1_787_435_821_375);

        // Round-trips against the epoch.
        assert_eq!(parse_timestamp("1970-01-01T00:00:00.000Z").unwrap(), 0);
    }

    #[test]
    fn tolerates_missing_or_short_fractions() {
        assert_eq!(
            parse_timestamp("2026-08-22T21:57:01Z").unwrap(),
            parse_timestamp("2026-08-22T21:57:01.000Z").unwrap()
        );
        // ".3" means 300ms, not 3ms.
        assert_eq!(
            parse_timestamp("2026-08-22T21:57:01.3Z").unwrap()
                - parse_timestamp("2026-08-22T21:57:01Z").unwrap(),
            300
        );
    }

    #[test]
    fn rejects_nonsense() {
        assert!(parse_timestamp("").is_none());
        assert!(parse_timestamp("not-a-timestamp").is_none());
    }

    #[test]
    fn truncate_collapses_whitespace_and_never_splits_a_character() {
        assert_eq!(truncate("hello   world", 40), "hello world");
        assert_eq!(truncate("abcdefghij", 4), "abcd…");
        // Counts characters, not bytes, so multi-byte text is not split.
        assert_eq!(truncate("ação rápida", 4), "ação…");
    }
}
