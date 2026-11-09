use super::Shared;
use crate::{
    discord::{ConnState, PresenceReport},
    error::{AppError, Result},
    session::{PresenceConfig, RecentGame, Session, SessionRecord},
    state::presence_defaults,
    util::now_ms,
};

#[tauri::command]
pub fn get_current_session(state: Shared<'_>) -> Option<Session> {
    crate::util::lock(&state.session).active.clone()
}

/// The presence a game would start with (game values + app-wide defaults).
#[tauri::command]
pub fn get_presence_defaults(state: Shared<'_>, game_id: String) -> Result<PresenceConfig> {
    let game = state.find_game(&game_id)?;
    Ok(presence_defaults(&game, &state.settings_snapshot()))
}

#[tauri::command]
pub fn get_presence_report(state: Shared<'_>) -> PresenceReport {
    state.report()
}

/// Starts playing `game_id`. Starting the game that is already running only
/// refreshes its presence; starting another game ends the current session.
#[tauri::command]
pub async fn start_game_session(
    state: Shared<'_>,
    game_id: String,
    presence: Option<PresenceConfig>,
) -> Result<Session> {
    let game = state.find_game(&game_id)?;
    let settings = state.settings_snapshot();
    let now = now_ms();
    let mut config = presence.unwrap_or_else(|| presence_defaults(&game, &settings));
    config.normalize();
    config.validate(now)?;

    let session = state.with_session(|store| store.start(&game, config, now))?;
    state.emit_session();

    // Starting to play should also show up on Discord: connect if the user
    // has an Application ID but has not connected yet.
    if !settings.application_id.is_empty() && state.connection_state() == ConnState::Idle {
        state.discord.connect(&settings.application_id);
    }
    state.sync_presence().await;
    Ok(session)
}

#[tauri::command]
pub async fn stop_game_session(state: Shared<'_>) -> Result<Option<SessionRecord>> {
    let record = state.with_session(|store| store.stop(now_ms()))?;
    state.emit_session();
    state.sync_presence().await;
    Ok(record)
}

/// Replaces the presence of the running session and re-shows it on Discord.
#[tauri::command]
pub async fn update_presence(state: Shared<'_>, presence: PresenceConfig) -> Result<Session> {
    let now = now_ms();
    let session = state.with_session(|store| store.update_presence(presence, now))??;
    state.emit_session();
    state.sync_presence().await;
    Ok(session)
}

/// Hides the presence on Discord without ending the session.
#[tauri::command]
pub async fn clear_presence(state: Shared<'_>) -> Result<Session> {
    let session = state
        .with_session(|store| store.set_visible(false))?
        .map_err(|_| AppError::NotFound("진행 중인 세션이 없습니다.".into()))?;
    state.emit_session();
    state.sync_presence().await;
    Ok(session)
}

#[tauri::command]
pub fn get_recent_games(state: Shared<'_>, limit: Option<usize>) -> Vec<RecentGame> {
    crate::util::lock(&state.session).recent_games(now_ms(), limit.unwrap_or(12))
}

#[tauri::command]
pub fn get_session_history(state: Shared<'_>, limit: Option<usize>) -> Vec<SessionRecord> {
    crate::util::lock(&state.session)
        .history
        .iter()
        .rev()
        .take(limit.unwrap_or(50))
        .cloned()
        .collect()
}
