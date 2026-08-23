//! Tauri commands for voice dictation (§54).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::OptionalExtension;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Manager, State};

use crate::AppState;

use super::capture::Recording;
use super::model::{self, DownloadProgress};
use super::prompt;
use super::server::ServerHandle;
use super::stream::{self, StreamEvent};
use super::transcribe;
use super::{Result, VoiceError};

/// Holds the one recording that can be in progress at a time, plus the
/// state that lives alongside it for live captions (§54 streaming, D31):
/// `server` is a warm `whisper-server.exe` kept alive across recordings
/// within one app session (its ~2-4s model-load cost is worth paying once,
/// not per utterance), and `streaming_stop` is the signal that tells the
/// current recording's poll thread to stop when a person stops or cancels.
///
/// A person has one microphone and one terminal focused at once — there is
/// no scenario where two overlapping recordings are the right behaviour, so
/// `active` is a slot, not a map.
#[derive(Default)]
pub struct VoiceState {
    active: Mutex<Option<Recording>>,
    server: Mutex<Option<Arc<ServerHandle>>>,
    streaming_stop: Mutex<Option<Arc<AtomicBool>>>,
}

/// Start the warm server if it is not already running, reusing it otherwise.
///
/// Returns `None` rather than an error when it cannot — live captions are a
/// bonus on top of dictation, not a requirement of it, so a missing model or
/// a server that fails to start must never stop a recording from working the
/// way it always has.
fn ensure_server(app: &tauri::AppHandle) -> Option<Arc<ServerHandle>> {
    let state = app.state::<AppState>();
    let mut guard = state.voice.server.lock();
    if let Some(existing) = guard.as_ref() {
        return Some(existing.clone());
    }

    let data_dir = app.path().app_data_dir().unwrap_or_default();
    let model_path = model::model_path(&data_dir);
    if !model_path.is_file() {
        return None;
    }
    let resource_dir = app.path().resource_dir().unwrap_or_default();

    match ServerHandle::start(&resource_dir, &model_path) {
        Ok(handle) => {
            let handle = Arc::new(handle);
            *guard = Some(handle.clone());
            Some(handle)
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not start whisper-server for live captions");
            None
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub present: bool,
}

#[tauri::command]
pub fn voice_model_status(app: tauri::AppHandle) -> ModelStatus {
    let data_dir = app.path().app_data_dir().unwrap_or_default();
    ModelStatus {
        present: model::is_present(&data_dir),
    }
}

/// Download the transcription model, reporting progress on `channel`.
///
/// Runs on its own thread so the ~490MB transfer never blocks the command
/// dispatcher; the command itself returns as soon as that thread is started,
/// and every subsequent step — including the final success or failure —
/// arrives as a message on `channel` instead of in this call's own result.
#[tauri::command]
pub fn voice_download_model(app: tauri::AppHandle, channel: Channel<DownloadProgress>) {
    let data_dir = app.path().app_data_dir().unwrap_or_default();
    std::thread::spawn(move || {
        model::download(&data_dir, &channel);
    });
}

/// Begin capturing, and — best-effort, never blocking this call — start
/// streaming live captions for it too.
///
/// `project_id` and `locale` are needed up front now (previously only
/// `voice_stop_recording` took them) because the streaming poll loop needs
/// the same vocabulary prompt and language the final transcription uses,
/// and it starts polling immediately rather than waiting for `stop`.
/// `channel` carries `StreamEvent::Partial` messages for as long as the
/// recording runs — see `stream::run_polling_loop`.
#[tauri::command]
pub fn voice_start_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    locale: String,
    channel: Channel<StreamEvent>,
) -> Result<()> {
    let mut active = state.voice.active.lock();
    if active.is_some() {
        return Err(VoiceError::AlreadyRecording);
    }
    let recording = Recording::start()?;
    let tap = recording.tap();
    *active = Some(recording);
    drop(active);

    let streaming_stop = Arc::new(AtomicBool::new(false));
    *state.voice.streaming_stop.lock() = Some(streaming_stop.clone());

    let project_path = project_root(&state, &project_id).unwrap_or_default();
    let vocabulary = prompt::build(&state.db, &project_id, &project_path);
    let whisper_language = transcribe::whisper_language_for(&locale).to_string();

    std::thread::spawn(move || {
        let Some(server) = ensure_server(&app) else {
            return;
        };
        stream::run_polling_loop(
            tap,
            server.base_url(),
            vocabulary,
            whisper_language,
            streaming_stop,
            channel,
        );
    });

    Ok(())
}

/// Stop recording without transcribing — the person changed their mind.
#[tauri::command]
pub fn voice_cancel_recording(state: State<'_, AppState>) -> Result<()> {
    let recording = state
        .voice
        .active
        .lock()
        .take()
        .ok_or(VoiceError::NotRecording)?;
    if let Some(flag) = state.voice.streaming_stop.lock().take() {
        flag.store(true, Ordering::SeqCst);
    }
    recording.cancel();
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceTranscript {
    pub text: String,
}

/// Stop recording, transcribe, and type the result into `session_id`'s
/// prompt — **not submitted**. It lands where a person can read it, edit it,
/// or throw it away with Ctrl+C, exactly as if they had typed it themselves
/// and paused before pressing Enter.
#[tauri::command]
pub fn voice_stop_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    session_id: String,
    locale: String,
) -> Result<VoiceTranscript> {
    // Fail on a stale session before spending any CPU on transcription.
    let session = state.sessions.get(&session_id)?;

    let recording = state
        .voice
        .active
        .lock()
        .take()
        .ok_or(VoiceError::NotRecording)?;
    if let Some(flag) = state.voice.streaming_stop.lock().take() {
        flag.store(true, Ordering::SeqCst);
    }
    let samples = recording.stop();

    let data_dir = app.path().app_data_dir().unwrap_or_default();
    let model_path = model::model_path(&data_dir);
    if !model_path.is_file() {
        return Err(VoiceError::ModelMissing);
    }

    let project_path = project_root(&state, &project_id)?;
    let vocabulary = prompt::build(&state.db, &project_id, &project_path);
    let resource_dir = app.path().resource_dir().unwrap_or_default();
    let language = transcribe::whisper_language_for(&locale);

    let wav_path = std::env::temp_dir().join(format!("jarvis-voice-{}.wav", uuid::Uuid::new_v4()));
    write_wav(&wav_path, &samples)?;

    let text = transcribe::transcribe(&resource_dir, &model_path, &wav_path, &vocabulary, language)?;

    if !text.is_empty() {
        crate::session::typing::type_text(&session, &text)
            .map_err(|_| VoiceError::Transcribe("could not type the transcript in".into()))?;
    }

    Ok(VoiceTranscript { text })
}

fn write_wav(path: &std::path::Path, samples: &[f32]) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: super::capture::TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::create(path, spec).map_err(|e| VoiceError::Transcribe(e.to_string()))?;
    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        writer
            .write_sample((clamped * i16::MAX as f32) as i16)
            .map_err(|e| VoiceError::Transcribe(e.to_string()))?;
    }
    writer
        .finalize()
        .map_err(|e| VoiceError::Transcribe(e.to_string()))?;
    Ok(())
}

fn project_root(state: &State<'_, AppState>, project_id: &str) -> Result<std::path::PathBuf> {
    let path: Option<String> = state.db.with(|conn| {
        conn.query_row(
            "SELECT path FROM projects WHERE id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .optional()
    })?;
    Ok(path.map(std::path::PathBuf::from).unwrap_or_default())
}
