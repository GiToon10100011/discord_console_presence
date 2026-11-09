use super::Shared;
use crate::{
    error::{AppError, Result},
    storage::{Game, ImageSource},
    util::lock,
};

#[tauri::command]
pub fn get_game_library(state: Shared<'_>) -> Vec<Game> {
    lock(&state.library).games.clone()
}

#[tauri::command]
pub fn add_game(state: Shared<'_>, game: Game) -> Result<Game> {
    state.with_library(|library| library.add(game))?
}

#[tauri::command]
pub fn update_game(state: Shared<'_>, game: Game) -> Result<Game> {
    state.with_library(|library| library.update(game))?
}

#[tauri::command]
pub fn toggle_favorite(state: Shared<'_>, game_id: String) -> Result<Game> {
    state.with_library(|library| {
        let mut game = library
            .find(&game_id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("게임을 찾을 수 없습니다: {game_id}")))?;
        game.favorite = !game.favorite;
        library.update(game)
    })?
}

/// Deletes a game. The game that is being played cannot be deleted, and an
/// uploaded image that no other game uses is removed with it.
#[tauri::command]
pub fn remove_game(state: Shared<'_>, game_id: String) -> Result<()> {
    if lock(&state.session)
        .active
        .as_ref()
        .is_some_and(|s| s.game_id == game_id)
    {
        return Err(AppError::Invalid(
            "플레이 중인 게임은 삭제할 수 없습니다. 먼저 세션을 종료하세요.".into(),
        ));
    }
    let removed = state.with_library(|library| library.remove(&game_id))??;

    if removed.image_source == ImageSource::Uploaded {
        if let Some(name) = removed.local_image_path.as_deref() {
            let still_used = lock(&state.library).games.iter().any(|g| {
                g.image_source == ImageSource::Uploaded
                    && g.local_image_path.as_deref() == Some(name)
            });
            if !still_used {
                if let Ok(path) = crate::image::local::uploaded_path(&state.paths, name) {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
    Ok(())
}
