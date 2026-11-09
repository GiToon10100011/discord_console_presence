use std::path::PathBuf;

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use super::Shared;
use crate::{
    error::{AppError, Result},
    image::{
        cache::{CacheEntry, CacheStats},
        cover::CoverCandidate,
        local, ImageView,
    },
    storage::{ImageSource, ImageSpec},
};

/// What the UI can show for an image setting (also used for unsaved edits).
#[tauri::command]
pub async fn get_image_view(state: Shared<'_>, spec: ImageSpec) -> Result<ImageView> {
    Ok(state.image_view(&spec).await)
}

#[tauri::command]
pub async fn get_game_image(state: Shared<'_>, game_id: String) -> Result<ImageView> {
    let game = state.find_game(&game_id)?;
    Ok(state.image_view(&game.image_spec()).await)
}

/// Downloads an external image into the cache (or reuses the cached copy).
/// `force` revalidates it against the server.
#[tauri::command]
pub async fn download_game_image(
    state: Shared<'_>,
    url: String,
    force: Option<bool>,
) -> Result<ImageView> {
    let view = state.url_view(url.trim(), force.unwrap_or(false)).await;
    match view.message {
        Some(message) if view.status == crate::image::ImageStatus::Error => {
            Err(AppError::Image(message))
        }
        _ => Ok(view),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheOverview {
    pub stats: CacheStats,
    pub entries: Vec<CacheEntry>,
}

#[tauri::command]
pub fn get_image_cache(state: Shared<'_>) -> CacheOverview {
    CacheOverview {
        stats: state.images.stats(),
        entries: state.images.list(),
    }
}

#[tauri::command]
pub fn clear_image_cache(state: Shared<'_>) -> Result<CacheStats> {
    state.images.clear()?;
    Ok(state.images.stats())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedImage {
    pub source: ImageSource,
    /// Value to store in the game's `localImagePath`.
    pub path: String,
    pub view: ImageView,
}

/// Brings a file from disk into the app. `uploaded` keeps a managed copy
/// (scaled to 1024px if larger); `local_file` only remembers the path.
#[tauri::command]
pub fn import_local_image(state: Shared<'_>, path: String, mode: ImageSource) -> Result<ImportedImage> {
    let source = PathBuf::from(&path);
    match mode {
        ImageSource::Uploaded => {
            let imported = local::import_uploaded(&state.paths, &state.images, &source)?;
            Ok(ImportedImage {
                source: mode,
                path: imported.file_name,
                view: ImageView::ready(&state.images, &imported.entry),
            })
        }
        ImageSource::LocalFile => {
            let entry = local::thumbnail_for_file(&state.images, &source)?;
            let absolute = source
                .canonicalize()
                .unwrap_or(source)
                .to_string_lossy()
                .into_owned();
            Ok(ImportedImage {
                source: mode,
                path: absolute,
                view: ImageView::ready(&state.images, &entry),
            })
        }
        _ => Err(AppError::Invalid(
            "로컬 이미지는 'uploaded' 또는 'local_file'로만 가져올 수 있습니다.".into(),
        )),
    }
}

#[tauri::command]
pub async fn search_game_cover(state: Shared<'_>, query: String) -> Result<Vec<CoverCandidate>> {
    state.covers.search(&state.http, &query).await
}

/// Opens the file manager at an image the app manages (e.g. to drag it into
/// the Developer Portal). Only the app's own image folders are allowed.
#[tauri::command]
pub fn reveal_path(app: AppHandle, state: Shared<'_>, path: String) -> Result<()> {
    let target = PathBuf::from(&path).canonicalize()?;
    let allowed = [state.paths.images_dir(), state.paths.uploads_dir()]
        .iter()
        .filter_map(|dir| dir.canonicalize().ok())
        .any(|dir| target.starts_with(dir));
    if !allowed {
        return Err(AppError::Invalid(
            "앱 이미지 폴더 안의 파일만 열 수 있습니다.".into(),
        ));
    }
    app.opener()
        .reveal_item_in_dir(&target)
        .map_err(|e| AppError::Io(std::io::Error::other(e.to_string())))
}
