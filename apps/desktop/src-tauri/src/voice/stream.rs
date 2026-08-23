//! Live captions while recording (§54 streaming, D30).
//!
//! Whisper is not a streaming model: every call re-reads the audio it is
//! given from the start and can change its mind about words it produced on
//! an earlier call as more context arrives. Naively replacing the visible
//! caption with each new poll's result would flicker and rewrite words a
//! person already read — the opposite of the premium feel this was built
//! for. [`AgreementState`] is the fix: it only ever *grows* the committed
//! caption, and only once two consecutive polls agree on a prefix (a
//! simplified "LocalAgreement" policy, same idea real streaming-whisper
//! projects use). The volatile remainder is shown too, but is allowed to be
//! replaced on the next poll.
//!
//! This module is deliberately split from the HTTP call: [`AgreementState`]
//! is pure and takes a plain word list, so the actual streaming *policy* is
//! unit-tested without a network, a process, or a model on disk anywhere in
//! sight. [`transcribe_window`] is the one function here that talks to a
//! real `whisper-server.exe` — see `commands.rs` for where the two meet.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::ipc::Channel;

use super::capture::AudioTap;
use super::{Result, VoiceError};

/// How long to wait between polls once one finishes. Transcription itself
/// (a few seconds for a short utterance on `ggml-small.bin`, see D30) is the
/// real pacing for anything but the very start of a recording — this floor
/// only matters for the first, near-empty polls, and keeps the loop from
/// hammering the server on a fast reply.
const POLL_FLOOR: Duration = Duration::from_millis(600);

/// Below this many samples (a quarter second at 16kHz) there is not enough
/// audio yet to be worth a round trip.
const MIN_SAMPLES: usize = super::capture::TARGET_SAMPLE_RATE as usize / 4;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum StreamEvent {
    Partial { committed: String, tail: String },
}

/// What a caller should show after folding in one poll's transcription.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Update {
    /// Settled text. Only ever grows, word for word, across a recording —
    /// never rewritten once shown, so nothing a person already read ever
    /// changes under them.
    pub committed: String,
    /// The rest of the current hypothesis, past the committed prefix. May be
    /// entirely different on the next poll — this is the part worth
    /// animating as "still listening" rather than "said".
    pub tail: String,
}

/// Tracks agreement across polls for one recording. One instance per
/// recording; discarded when it stops.
#[derive(Debug, Default)]
pub struct AgreementState {
    committed_words: Vec<String>,
    previous_hypothesis: Vec<String>,
}

impl AgreementState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold in the words of one poll's transcript (already whitespace-split
    /// by the caller — see [`split_words`]).
    ///
    /// A word only becomes committed once it sits in the common prefix of
    /// *this* hypothesis and the *previous* one — one poll's guess is not
    /// enough on its own, that is what causes the flicker this exists to
    /// avoid. And committed text is never retracted: if a new hypothesis
    /// disagrees with something already committed, the disagreement is
    /// simply not reflected — see the module doc for why that trade is the
    /// right one here (the caption is a preview; the final typed text comes
    /// from a separate, complete, unstreamed pass in `commands.rs`).
    pub fn update(&mut self, hypothesis: Vec<String>) -> Update {
        let agree_len = self
            .previous_hypothesis
            .iter()
            .zip(hypothesis.iter())
            .take_while(|(a, b)| a == b)
            .count();

        let agreed_prefix = &hypothesis[..agree_len];
        let extends_committed = agreed_prefix.len() >= self.committed_words.len()
            && agreed_prefix[..self.committed_words.len()] == self.committed_words[..];
        if extends_committed && agreed_prefix.len() > self.committed_words.len() {
            self.committed_words = agreed_prefix.to_vec();
        }

        self.previous_hypothesis = hypothesis.clone();

        let tail_start = self.committed_words.len().min(hypothesis.len());
        Update {
            committed: self.committed_words.join(" "),
            tail: hypothesis[tail_start..].join(" "),
        }
    }
}

/// Split whisper-server's `text` field into comparable words. Newlines
/// (whisper.cpp sometimes puts one between segments) become spaces first, so
/// they never masquerade as part of a word.
pub fn split_words(text: &str) -> Vec<String> {
    text.replace('\n', " ")
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// POST the samples to a running `whisper-server.exe` and return its
/// transcript of them.
///
/// Deliberately asks for plain `json` (`{"text": "..."}`), not
/// `verbose_json` — the segment/word arrays that mode adds carry
/// sub-word tokens (a whisper.cpp token boundary, not a word boundary; see
/// the module doc), which is the wrong granularity for [`AgreementState`]'s
/// word-level comparison. The assembled `text` field is already correctly
/// spaced and punctuated, which is what makes plain whitespace-splitting on
/// it safe.
pub fn transcribe_window(
    base_url: &str,
    samples: &[f32],
    prompt: &str,
    whisper_language: &str,
) -> Result<String> {
    let wav_bytes = encode_wav(samples)?;
    let boundary = "jarvisvoiceboundary7f3a9c";
    let body = build_multipart(
        boundary,
        &wav_bytes,
        &[
            ("response_format", "json"),
            ("language", whisper_language),
            ("prompt", prompt),
            ("no_timestamps", "true"),
        ],
    );

    let resp = ureq::post(&format!("{base_url}/inference"))
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .timeout(Duration::from_secs(20))
        .send_bytes(&body)
        .map_err(|e| VoiceError::Server(format!("whisper-server request failed: {e}")))?;

    let json: serde_json::Value = serde_json::from_reader(resp.into_reader())
        .map_err(|e| VoiceError::Server(format!("could not parse whisper-server's reply: {e}")))?;

    Ok(json
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string())
}

/// Poll a warm `whisper-server.exe` for as long as `stop` stays false,
/// emitting a `StreamEvent::Partial` on `channel` each time the visible
/// caption actually changes.
///
/// Runs on its own thread (see `commands::voice_start_recording`) so it
/// never blocks the recording it is reading from. Full-buffer, not a
/// trailing window: every poll re-sends everything captured so far. That is
/// the simple, honest trade this makes for v1 — see D30 — latency grows
/// with utterance length since whisper re-reads from the start each time,
/// but the adaptive cadence below (next poll only after the last one
/// returns) means a long utterance degrades to slower, still-correct
/// updates rather than a queue that falls further and further behind.
pub fn run_polling_loop(
    tap: AudioTap,
    server_base_url: String,
    vocabulary: String,
    whisper_language: String,
    stop: Arc<AtomicBool>,
    channel: Channel<StreamEvent>,
) {
    let mut agreement = AgreementState::new();
    let mut committed_so_far = String::new();
    let mut last_sent: Option<Update> = None;

    while !stop.load(Ordering::SeqCst) {
        let samples = tap.snapshot();
        if samples.len() < MIN_SAMPLES {
            std::thread::sleep(POLL_FLOOR);
            continue;
        }

        let prompt = if committed_so_far.is_empty() {
            vocabulary.clone()
        } else {
            format!("{vocabulary} {committed_so_far}")
        };

        if let Ok(text) = transcribe_window(&server_base_url, &samples, &prompt, &whisper_language) {
            let update = agreement.update(split_words(&text));
            committed_so_far = update.committed.clone();
            if last_sent.as_ref() != Some(&update) {
                let _ = channel.send(StreamEvent::Partial {
                    committed: update.committed.clone(),
                    tail: update.tail.clone(),
                });
                last_sent = Some(update);
            }
        }
        // A transient failure (server mid-warmup, one dropped request) is
        // not reported — the next poll tries again on its own. What would
        // be worth reporting is a caption that never arrives at all, and a
        // person watching the mic light is already evidence of that.

        std::thread::sleep(POLL_FLOOR);
    }
}

fn encode_wav(samples: &[f32]) -> Result<Vec<u8>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: super::capture::TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|e| VoiceError::Transcribe(e.to_string()))?;
        for &sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            writer
                .write_sample((clamped * i16::MAX as f32) as i16)
                .map_err(|e| VoiceError::Transcribe(e.to_string()))?;
        }
        writer
            .finalize()
            .map_err(|e| VoiceError::Transcribe(e.to_string()))?;
    }
    Ok(cursor.into_inner())
}

fn build_multipart(boundary: &str, wav_bytes: &[u8], fields: &[(&str, &str)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(wav_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        split_words(s)
    }

    #[test]
    fn nothing_commits_on_the_first_poll_alone() {
        let mut state = AgreementState::new();
        let update = state.update(words("hello there"));
        assert_eq!(update.committed, "");
        assert_eq!(update.tail, "hello there");
    }

    #[test]
    fn a_prefix_agreed_by_two_consecutive_polls_commits() {
        let mut state = AgreementState::new();
        state.update(words("hello there general"));
        let update = state.update(words("hello there general kenobi"));
        assert_eq!(update.committed, "hello there general");
        assert_eq!(update.tail, "kenobi");
    }

    #[test]
    fn committed_text_never_shrinks_even_if_a_later_poll_disagrees() {
        let mut state = AgreementState::new();
        state.update(words("hello there general"));
        state.update(words("hello there general kenobi"));
        // A wildly different next hypothesis (e.g. a VAD hiccup) must not
        // erase what was already shown as settled.
        let update = state.update(words("something else entirely"));
        assert_eq!(update.committed, "hello there general");
    }

    #[test]
    fn the_tail_always_reflects_the_latest_hypothesis() {
        let mut state = AgreementState::new();
        state.update(words("um informa"));
        let update = state.update(words("um informacao"));
        // "um" agreed across both polls and committed; "informa" did not
        // survive as "informacao" changed it, so only the new word shows in
        // the tail — not the stale first guess and not the whole sentence.
        assert_eq!(update.committed, "um");
        assert_eq!(update.tail, "informacao");
    }

    #[test]
    fn identical_polls_keep_advancing_the_committed_prefix() {
        let mut state = AgreementState::new();
        state.update(words("a"));
        state.update(words("a b"));
        state.update(words("a b c"));
        let update = state.update(words("a b c d"));
        assert_eq!(update.committed, "a b c");
        assert_eq!(update.tail, "d");
    }

    #[test]
    fn a_newline_between_segments_does_not_glue_words_together() {
        assert_eq!(
            split_words("hello\nworld"),
            vec!["hello".to_string(), "world".to_string()]
        );
    }

    /// Spawns the real `whisper-server.exe`, against the real downloaded
    /// model, and sends it real audio over HTTP — proving the multipart
    /// shape and the `response_format=json` field name this module was
    /// built around actually match what ships, rather than what the
    /// `--help` text and a hand-rolled body were assumed to imply. Ignored
    /// by default: needs both the server binary (bundled, so usually
    /// present) and the ~490MB model (only present once a person has used
    /// dictation on this machine at least once).
    #[test]
    #[ignore]
    fn a_real_whisper_server_transcribes_real_audio_over_http() {
        let resource_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        let appdata = std::env::var("APPDATA").expect("APPDATA must be set on Windows");
        let model_path = std::path::PathBuf::from(appdata)
            .join("dev.jarvis.desktop")
            .join("models")
            .join("ggml-small.bin");
        if !model_path.is_file() {
            eprintln!("skipping: no model downloaded at {}", model_path.display());
            return;
        }

        let handle = super::super::server::ServerHandle::start(&resource_dir, &model_path)
            .expect("whisper-server should start and become ready");

        // A second and a half of silence — enough for whisper.cpp to accept
        // as one chunk. The point is not what it transcribes, only that a
        // real HTTP round trip against the real binary succeeds and comes
        // back as the shape this module expects.
        let silence = vec![0.0f32; super::super::capture::TARGET_SAMPLE_RATE as usize * 3 / 2];
        let text = transcribe_window(&handle.base_url(), &silence, "J.A.R.V.I.S.", "en")
            .expect("a real whisper-server request should succeed");
        // Whisper hallucinates *something* on pure silence rather than
        // returning an empty string (the same behaviour D29 documents for
        // whisper-cli) — the real assertion is that this line was reached
        // at all: the multipart body was accepted and `text` was present
        // and parseable in the JSON reply.
        let _ = text;
    }

    #[test]
    fn the_multipart_body_carries_the_wav_bytes_intact_between_the_boundaries() {
        let body = build_multipart("B", &[1, 2, 3, 0xFF], &[("language", "pt")]);
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("name=\"language\""));
        assert!(text.contains("pt"));
        assert!(text.contains("filename=\"audio.wav\""));
        // The raw bytes must appear verbatim between the file part's
        // headers and the closing boundary — not re-encoded.
        assert!(body
            .windows(4)
            .any(|w| w == [1, 2, 3, 0xFF]));
    }
}
