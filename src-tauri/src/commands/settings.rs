use serde::Serialize;
use tauri_plugin_autostart::ManagerExt;

use super::Shared;
use crate::{
    discord::ConnState,
    error::{AppError, Result},
    storage::Settings,
};

const MB: u64 = 1024 * 1024;

#[tauri::command]
pub fn get_settings(state: Shared<'_>) -> Settings {
    state.settings_snapshot()
}

/// Validates and saves the settings and applies everything that takes effect
/// immediately: autostart, the cache budget, the Discord connection and the
/// presence (the fallback image and name override are part of it).
#[tauri::command]
pub async fn update_settings(state: Shared<'_>, settings: Settings) -> Result<Settings> {
    let mut next = settings;
    next.sanitize()?;
    // Fail here, with a clear message, instead of at the first export.
    if !next.export_dir.is_empty() {
        std::fs::create_dir_all(&next.export_dir).map_err(|e| {
            AppError::Invalid(format!("내보내기 폴더를 사용할 수 없습니다: {e}"))
        })?;
    }
    let previous = state.settings_snapshot();

    if previous.launch_at_login != next.launch_at_login {
        let launcher = state.app.autolaunch();
        let applied = if next.launch_at_login {
            launcher.enable()
        } else {
            launcher.disable()
        };
        applied.map_err(|e| {
            AppError::Storage(format!("로그인 시 자동 실행을 변경하지 못했습니다: {e}"))
        })?;
    }

    state.with_settings(|s| *s = next.clone())?;
    state
        .images
        .set_max_bytes(u64::from(next.image_cache_max_mb) * MB);

    if previous.application_id != next.application_id {
        state.assets.invalidate();
        if next.application_id.is_empty() {
            state.discord.disconnect();
        } else if next.auto_connect || state.connection_state() != ConnState::Idle {
            state.discord.connect(&next.application_id);
        }
    }

    let presence_changed = previous.application_id != next.application_id
        || previous.fallback_asset_key != next.fallback_asset_key
        || previous.fallback_image_url != next.fallback_image_url
        || previous.override_activity_name != next.override_activity_name;
    if presence_changed {
        state.sync_presence().await;
    }
    Ok(next)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub os: &'static str,
    pub data_dir: String,
    pub cache_dir: String,
    /// Where cropped images go when the settings name no folder.
    pub default_export_dir: String,
    /// Where cropped images go right now.
    pub export_dir: String,
}

#[tauri::command]
pub fn get_app_info(state: Shared<'_>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS,
        data_dir: state.paths.data_dir.to_string_lossy().into_owned(),
        cache_dir: state.paths.cache_dir.to_string_lossy().into_owned(),
        default_export_dir: state.default_export_dir().to_string_lossy().into_owned(),
        export_dir: state.export_dir().to_string_lossy().into_owned(),
    }
}
