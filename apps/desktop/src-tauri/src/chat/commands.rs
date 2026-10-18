//! What the chat screen can ask of a chat session (M29). Refusals are stable
//! codes the surface localises (§65).

use tauri::ipc::Channel;
use tauri::State;

use super::model::{ChatAnswer, ChatEvent};
use super::Snapshot;
use crate::AppState;

/// Start streaming a chat to a screen, returning everything already known.
#[tauri::command]
pub fn chat_attach(
    state: State<'_, AppState>,
    session_id: String,
    attachment_id: String,
    channel: Channel<ChatEvent>,
) -> Result<Snapshot, String> {
    let runtime = state.chats.get(&session_id)?;
    let snapshot = runtime.attach(attachment_id, channel);
    // Somebody is looking, so a guardrail that wants a human has one (§35).
    crate::guardrail::sessions::set_attended(&state.session_dir(&session_id), true);
    Ok(snapshot)
}

#[tauri::command]
pub fn chat_detach(state: State<'_, AppState>, session_id: String, attachment_id: String) -> Result<(), String> {
    let runtime = state.chats.get(&session_id)?;
    if runtime.detach(&attachment_id) {
        // Nobody to ask any more: an `ask` rule refuses instead of hanging (D12).
        crate::guardrail::sessions::set_attended(&state.session_dir(&session_id), false);
    }
    Ok(())
}

#[tauri::command]
pub fn chat_send(state: State<'_, AppState>, session_id: String, text: String) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("chat.empty".into());
    }
    let runtime = state.chats.get(&session_id)?;
    crate::session::commands::record_interaction(&state, &session_id);
    runtime.run(|driver| Ok(driver.send(text)))
}

#[tauri::command]
pub fn chat_answer(
    state: State<'_, AppState>,
    session_id: String,
    request_id: String,
    answer: ChatAnswer,
) -> Result<(), String> {
    let runtime = state.chats.get(&session_id)?;
    crate::session::commands::record_interaction(&state, &session_id);
    runtime.run(|driver| driver.answer(&request_id, answer))
}

#[tauri::command]
pub fn chat_set_model(state: State<'_, AppState>, session_id: String, model: String) -> Result<(), String> {
    state.chats.get(&session_id)?.run(|driver| Ok(driver.set_model(&model)))
}

#[tauri::command]
pub fn chat_set_effort(state: State<'_, AppState>, session_id: String, effort: String) -> Result<(), String> {
    state.chats.get(&session_id)?.run(|driver| Ok(driver.set_effort(&effort)))
}

#[tauri::command]
pub fn chat_interrupt(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    state.chats.get(&session_id)?.run(|driver| Ok(driver.interrupt()))
}
