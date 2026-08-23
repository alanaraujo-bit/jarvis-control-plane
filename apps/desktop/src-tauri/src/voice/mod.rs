//! Voice dictation (§54) — speak into the terminal instead of typing.
//!
//! Fully local: audio never leaves the machine. Captured with `cpal`,
//! transcribed by a bundled `whisper-cli.exe` (whisper.cpp) run against a
//! model downloaded once on first use, then typed into the live session
//! through the same paced path autopilot instructions use
//! (`session::typing`) — landed in the prompt for a person to read, never
//! auto-submitted (see `commands::voice_stop_recording`).
//!
//! Why local rather than a cloud transcription API, and why whisper.cpp
//! rather than Windows' own dictation: see D29 in docs/DECISIONS.md. Short
//! version — no API key to manage, no per-utterance cost, and it keeps to the
//! same local-first boundary (§3) as everything else session data touches.
//! The trade is a real deployment dependency (`whisper-cli.exe` needs the
//! VC++ runtime, bundled alongside it in `resources/whisper/`, app-local, not
//! a system install) and a one-time ~490MB model download.
//!
//! While a recording is in progress, `stream` and `server` drive a second,
//! independent path: a warm `whisper-server.exe` is polled every second or
//! two over the audio captured so far, and `stream::AgreementState` turns
//! that into a caption that only ever grows, never rewritten mid-word — see
//! D31. That path never touches what gets typed; the transcript that lands
//! in the prompt still comes from the single, complete, unstreamed
//! `whisper-cli.exe` pass in `commands::voice_stop_recording`, unchanged.

pub mod capture;
pub mod commands;
pub mod model;
pub mod prompt;
pub mod server;
pub mod stream;
pub mod transcribe;

pub use commands::*;

#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    #[error("no microphone is available")]
    NoInputDevice,
    #[error("a recording is already in progress")]
    AlreadyRecording,
    #[error("nothing is being recorded")]
    NotRecording,
    #[error("audio device error: {0}")]
    Device(String),
    #[error("the transcription model has not been downloaded yet")]
    ModelMissing,
    #[error("could not download the model: {0}")]
    Download(String),
    #[error("the downloaded model failed verification")]
    ModelCorrupt,
    #[error("could not transcribe: {0}")]
    Transcribe(String),
    #[error("live transcription server error: {0}")]
    Server(String),
    #[error("{0}")]
    Session(#[from] crate::session::manager::SessionError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] crate::db::DbError),
}

impl serde::Serialize for VoiceError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, VoiceError>;
