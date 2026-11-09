use serde::Serialize;

use super::Shared;
use crate::{
    discord::{probe, DiscordStatus, DiscordUser},
    error::{AppError, Result},
    image::assets::AppAsset,
    storage::Settings,
};

#[tauri::command]
pub fn get_discord_status(state: Shared<'_>) -> DiscordStatus {
    state.discord.status()
}

/// Connects to the local Discord client. A new `application_id` is validated
/// and saved first, so "save and connect" is a single step in the UI.
#[tauri::command]
pub async fn connect_discord(
    state: Shared<'_>,
    application_id: Option<String>,
) -> Result<DiscordStatus> {
    if let Some(id) = application_id {
        let mut next: Settings = state.settings_snapshot();
        next.application_id = id;
        next.sanitize()?;
        if next.application_id != state.settings_snapshot().application_id {
            state.assets.invalidate();
            let id = next.application_id.clone();
            state.with_settings(|s| s.application_id = id)?;
        }
    }
    state.connect_discord()?;
    state.sync_presence().await;
    Ok(state.discord.status())
}

#[tauri::command]
pub fn disconnect_discord(state: Shared<'_>) -> DiscordStatus {
    state.discord.disconnect();
    state.discord.status()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTest {
    pub ok: bool,
    pub message: String,
    pub user: Option<DiscordUser>,
    /// Number of Rich Presence assets registered for the application.
    pub asset_count: Option<usize>,
    pub asset_error: Option<String>,
}

/// One-off check of Discord + Application ID. It never touches the live
/// connection or the current presence.
#[tauri::command]
pub async fn test_discord_connection(
    state: Shared<'_>,
    application_id: Option<String>,
) -> Result<ConnectionTest> {
    let id = application_id
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| state.settings_snapshot().application_id);
    if id.is_empty() {
        return Err(AppError::Invalid("Application ID를 입력하세요.".into()));
    }
    let mut check = Settings::default();
    check.application_id = id.clone();
    check.sanitize()?;

    let probe_id = id.clone();
    let probed = tauri::async_runtime::spawn_blocking(move || probe(&probe_id))
        .await
        .map_err(|e| AppError::Discord(format!("연결 확인이 중단되었습니다: {e}")))?;
    let user = match probed {
        Ok(user) => user,
        Err(message) => {
            return Ok(ConnectionTest {
                ok: false,
                message,
                user: None,
                asset_count: None,
                asset_error: None,
            })
        }
    };

    let (asset_count, asset_error) = match state.assets.list(&state.http, &id, true).await {
        Ok(assets) => (Some(assets.len()), None),
        Err(e) => (None, Some(e.to_string())),
    };
    Ok(ConnectionTest {
        ok: true,
        message: format!(
            "Discord에 연결되었습니다 ({}).",
            user.global_name.as_deref().unwrap_or(&user.username)
        ),
        user: Some(user),
        asset_count,
        asset_error,
    })
}

#[tauri::command]
pub async fn list_discord_assets(
    state: Shared<'_>,
    refresh: Option<bool>,
) -> Result<Vec<AppAsset>> {
    let id = state.settings_snapshot().application_id;
    state
        .assets
        .list(&state.http, &id, refresh.unwrap_or(false))
        .await
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetStatus {
    Registered,
    Missing,
    /// The asset list could not be read, so nothing can be said.
    Unknown,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetCheck {
    pub status: AssetStatus,
    pub message: String,
}

/// Is `key` registered for the configured application?
#[tauri::command]
pub async fn check_asset_key(state: Shared<'_>, key: String) -> Result<AssetCheck> {
    let key = key.trim().to_lowercase();
    if key.is_empty() {
        return Err(AppError::Invalid("Asset Key를 입력하세요.".into()));
    }
    let id = state.settings_snapshot().application_id;
    if id.is_empty() {
        return Ok(AssetCheck {
            status: AssetStatus::Unknown,
            message: "Application ID를 설정하면 등록 여부를 확인할 수 있습니다.".into(),
        });
    }
    Ok(match state.assets.list(&state.http, &id, false).await {
        Ok(assets) if assets.iter().any(|a| a.name.to_lowercase() == key) => AssetCheck {
            status: AssetStatus::Registered,
            message: "Developer Portal에 등록되어 있습니다.".into(),
        },
        Ok(_) => AssetCheck {
            status: AssetStatus::Missing,
            message: format!(
                "'{key}'는 이 앱에 등록돼 있지 않습니다. Developer Portal → Rich Presence → Art Assets에서 \
                 이 이름으로 업로드하세요 (키는 소문자로 저장됩니다)."
            ),
        },
        Err(e) => AssetCheck {
            status: AssetStatus::Unknown,
            message: e.to_string(),
        },
    })
}
