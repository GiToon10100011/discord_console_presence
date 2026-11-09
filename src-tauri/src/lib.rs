pub mod commands;
pub mod discord;
pub mod error;
pub mod image;
pub mod session;
pub mod state;
pub mod storage;
pub mod util;

use std::{sync::Arc, time::Duration};

use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            let state = Arc::new(AppState::new(app.handle())?);
            app.manage(state.clone());

            // A session left over from the last run is resumed or closed
            // before anything is shown on Discord.
            state.restore_session();
            let settings = state.settings_snapshot();
            if settings.auto_connect && !settings.application_id.is_empty() {
                state.discord.connect(&settings.application_id);
            }

            tauri::async_runtime::spawn(async move {
                state.sync_presence().await;
                loop {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    state.tick().await;
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::discord::get_discord_status,
            commands::discord::connect_discord,
            commands::discord::disconnect_discord,
            commands::discord::test_discord_connection,
            commands::discord::list_discord_assets,
            commands::discord::check_asset_key,
            commands::session::get_current_session,
            commands::session::get_presence_defaults,
            commands::session::get_presence_report,
            commands::session::start_game_session,
            commands::session::stop_game_session,
            commands::session::update_presence,
            commands::session::clear_presence,
            commands::session::get_recent_games,
            commands::session::get_session_history,
            commands::library::get_game_library,
            commands::library::add_game,
            commands::library::update_game,
            commands::library::toggle_favorite,
            commands::library::remove_game,
            commands::images::get_image_view,
            commands::images::get_game_image,
            commands::images::download_game_image,
            commands::images::get_image_cache,
            commands::images::clear_image_cache,
            commands::images::import_local_image,
            commands::images::search_game_cover,
            commands::images::export_cropped_image,
            commands::images::open_export_dir,
            commands::images::reveal_path,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::get_app_info,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the Tauri application");

    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = handle.try_state::<Arc<AppState>>() {
                state.shutdown();
            }
        }
    });
}
