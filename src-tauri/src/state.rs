use std::{path::Path, sync::Mutex, time::Duration};

use tauri::{AppHandle, Emitter, Manager};

use crate::{
    discord::{build_activity, ConnState, DiscordHandle, PresenceReport, WorkerConfig},
    error::{AppError, Result},
    image::{
        self,
        assets::{asset_image_url, AssetCatalog},
        cache::{self, ImageCache},
        cover::CoverFinder,
        local, ImageView,
    },
    session::{PresenceConfig, SessionStore},
    storage::{AppPaths, Game, ImageSource, ImageSpec, JsonStore, Library, Settings},
    util::{lock, now_ms},
};

pub const EVENT_DISCORD: &str = "discord://status";
pub const EVENT_SESSION: &str = "session://changed";
pub const EVENT_REPORT: &str = "presence://report";

const MB: u64 = 1024 * 1024;

/// Everything the commands share. The Discord connection (`discord`) and the
/// game session (`session`) are separate on purpose: the session only states
/// what should be on Discord (`sync_presence`), the worker deals with whether
/// Discord is reachable.
pub struct AppState {
    pub app: AppHandle,
    pub paths: AppPaths,
    pub http: reqwest::Client,
    pub settings: Mutex<Settings>,
    settings_store: JsonStore<Settings>,
    pub library: Mutex<Library>,
    library_store: JsonStore<Library>,
    pub session: Mutex<SessionStore>,
    session_store: JsonStore<SessionStore>,
    pub discord: DiscordHandle,
    pub images: ImageCache,
    pub assets: AssetCatalog,
    pub covers: CoverFinder,
    report: Mutex<PresenceReport>,
    /// Used when the settings do not name an export folder.
    default_export_dir: std::path::PathBuf,
}

/// The folder cropped images go to: the one from the settings, or the default.
pub fn effective_export_dir(configured: &str, default: &Path) -> std::path::PathBuf {
    let configured = configured.trim();
    if configured.is_empty() {
        default.to_path_buf()
    } else {
        std::path::PathBuf::from(configured)
    }
}

/// The presence a game starts with: its own values, then the app-wide
/// defaults for anything it leaves blank.
pub fn presence_defaults(game: &Game, settings: &Settings) -> PresenceConfig {
    let pick = |own: &str, fallback: &str| {
        if own.trim().is_empty() {
            fallback.to_string()
        } else {
            own.to_string()
        }
    };
    let mut config = PresenceConfig {
        game_name: game.name.clone(),
        details: pick(&game.details, &settings.default_details),
        state: pick(&game.state, &settings.default_state),
        large_image: game.image_spec(),
        // The platform has no field of its own on the card; the hover text of
        // the large image is where it is shown.
        large_image_text: game
            .large_image_text
            .clone()
            .unwrap_or_else(|| format!("{} · {}", game.name, game.platform)),
        small_image: game.small_image.clone().unwrap_or_default(),
        small_image_text: game
            .small_image_text
            .clone()
            .unwrap_or_else(|| game.platform.clone()),
        start_timestamp: None,
    };
    config.normalize();
    config
}

impl AppState {
    pub fn new(app: &AppHandle) -> Result<Self> {
        let path_error = |e: tauri::Error| AppError::Storage(e.to_string());
        let paths = AppPaths {
            data_dir: app.path().app_data_dir().map_err(path_error)?,
            cache_dir: app.path().app_cache_dir().map_err(path_error)?,
        };
        paths.ensure()?;
        // Pictures/Console Presence is easy to find; the app's own data folder
        // is hidden (Library on macOS, AppData on Windows).
        let default_export_dir = app
            .path()
            .picture_dir()
            .map(|dir| dir.join("Console Presence"))
            .unwrap_or_else(|_| paths.exports_dir());

        let settings_store = JsonStore::<Settings>::new(paths.settings_file());
        let mut settings = settings_store.load();
        if let Err(e) = settings.sanitize() {
            log::warn!("stored settings are invalid ({e}); using defaults");
            settings = Settings::default();
        }

        let library_store = JsonStore::<Library>::new(paths.library_file());
        let mut library = library_store.load();
        if library.seed_defaults() {
            library_store.save(&library)?;
        }

        let session_store = JsonStore::<SessionStore>::new(paths.session_file());
        let session = session_store.load();

        let images = ImageCache::open(
            paths.images_dir(),
            u64::from(settings.image_cache_max_mb) * MB,
        );
        let events = app.clone();
        let discord = DiscordHandle::spawn(WorkerConfig::default(), move |status| {
            let _ = events.emit(EVENT_DISCORD, status);
        });

        Ok(Self {
            app: app.clone(),
            paths,
            http: image::http_client()?,
            settings: Mutex::new(settings),
            settings_store,
            library: Mutex::new(library),
            library_store,
            session: Mutex::new(session),
            session_store,
            discord,
            images,
            assets: AssetCatalog::default(),
            covers: CoverFinder::default(),
            report: Mutex::new(PresenceReport::default()),
            default_export_dir,
        })
    }

    // ---- persistence: mutate under the lock, save before releasing it ----

    pub fn with_settings<R>(&self, change: impl FnOnce(&mut Settings) -> R) -> Result<R> {
        let mut settings = lock(&self.settings);
        let out = change(&mut settings);
        self.settings_store.save(&settings)?;
        Ok(out)
    }

    pub fn with_library<R>(&self, change: impl FnOnce(&mut Library) -> R) -> Result<R> {
        let mut library = lock(&self.library);
        let out = change(&mut library);
        self.library_store.save(&library)?;
        Ok(out)
    }

    pub fn with_session<R>(&self, change: impl FnOnce(&mut SessionStore) -> R) -> Result<R> {
        let mut session = lock(&self.session);
        let out = change(&mut session);
        self.session_store.save(&session)?;
        Ok(out)
    }

    pub fn settings_snapshot(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn find_game(&self, id: &str) -> Result<Game> {
        lock(&self.library)
            .find(id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("게임을 찾을 수 없습니다: {id}")))
    }

    // ---- events ----

    pub fn emit_session(&self) {
        let active = lock(&self.session).active.clone();
        let _ = self.app.emit(EVENT_SESSION, active);
    }

    fn publish_report(&self, report: PresenceReport) {
        *lock(&self.report) = report.clone();
        let _ = self.app.emit(EVENT_REPORT, report);
    }

    pub fn report(&self) -> PresenceReport {
        lock(&self.report).clone()
    }

    // ---- Discord ----

    /// Starts connecting with the configured Application ID.
    pub fn connect_discord(&self) -> Result<()> {
        let id = self.settings_snapshot().application_id;
        if id.is_empty() {
            return Err(AppError::Invalid(
                "Application ID를 먼저 설정하세요 (설정 → Discord).".into(),
            ));
        }
        self.discord.connect(&id);
        Ok(())
    }

    /// Makes Discord show what the active session says it should (or clear
    /// it). Safe to call at any time: if Discord is not connected right now
    /// the worker keeps the result and applies it once it is.
    pub async fn sync_presence(&self) {
        let session = lock(&self.session).active.clone();
        let settings = self.settings_snapshot();
        let activity = match session {
            Some(session) if session.presence_visible => {
                let catalog = self.assets.lookup(&self.http, &settings.application_id).await;
                let (payload, report) = build_activity(
                    &session.presence,
                    session.effective_start(),
                    &settings,
                    &catalog,
                );
                self.publish_report(report);
                Some(payload)
            }
            _ => {
                self.publish_report(PresenceReport::default());
                None
            }
        };
        self.discord.set_activity(activity);
    }

    // ---- lifecycle ----

    /// Decides what happens to a session left over from the previous run.
    pub fn restore_session(&self) {
        let settings = self.settings_snapshot();
        let now = now_ms();
        let result = self.with_session(|store| {
            let Some(active) = store.active.clone() else {
                return;
            };
            let limit = (settings.auto_clear_hours > 0)
                .then(|| i64::from(settings.auto_clear_hours) * 3_600_000);
            let expired = limit.is_some_and(|l| now - active.started_at > l);
            if settings.restore_last_session && !expired {
                return;
            }
            // Close it where the app last saw it, not at "now".
            let mut end = active.last_seen_at.max(active.started_at);
            if let (true, Some(limit)) = (expired, limit) {
                end = end.min(active.started_at + limit);
            }
            store.stop(end.min(now));
        });
        if let Err(e) = result {
            log::error!("could not restore the session: {e}");
        }
    }

    /// Runs every ~30 s: heartbeat for crash recovery and the auto-clear limit.
    pub async fn tick(&self) {
        let settings = self.settings_snapshot();
        let now = now_ms();
        let limit = (settings.auto_clear_hours > 0)
            .then(|| i64::from(settings.auto_clear_hours) * 3_600_000);
        let ended = self.with_session(|store| {
            let expired = store
                .active
                .as_ref()
                .is_some_and(|a| limit.is_some_and(|l| now - a.started_at > l));
            if expired {
                store.stop(now);
                return true;
            }
            store.touch(now);
            false
        });
        match ended {
            Ok(true) => {
                self.emit_session();
                self.sync_presence().await;
            }
            Ok(false) => {}
            Err(e) => log::warn!("session heartbeat failed: {e}"),
        }
    }

    /// Keeps the running session on disk (it is restored next time) and
    /// releases the Discord connection.
    pub fn shutdown(&self) {
        let _ = self.with_session(|store| store.touch(now_ms()));
        self.discord.shutdown(Duration::from_secs(2));
    }

    // ---- images ----

    /// Resolves what the UI can show for an image setting.
    pub async fn image_view(&self, spec: &ImageSpec) -> ImageView {
        match spec.source {
            ImageSource::Default => ImageView::none(),
            ImageSource::DiscordAsset => self.asset_view(spec.asset_key.as_deref()).await,
            ImageSource::ExternalUrl => match spec.url.as_deref().map(str::trim) {
                None | Some("") => ImageView::none(),
                Some(url) => self.url_view(url, false).await,
            },
            ImageSource::Uploaded => match spec.local_path.as_deref().map(str::trim) {
                None | Some("") => ImageView::none(),
                Some(name) => match local::uploaded_path(&self.paths, name) {
                    Ok(path) => self.file_view(&path),
                    Err(e) => ImageView::error(e.to_string()),
                },
            },
            ImageSource::LocalFile => match spec.local_path.as_deref().map(str::trim) {
                None | Some("") => ImageView::none(),
                Some(path) => self.file_view(Path::new(path)),
            },
        }
    }

    pub async fn url_view(&self, url: &str, force: bool) -> ImageView {
        match cache::fetch_with(&self.http, &self.images, url, force, &Default::default()).await {
            Ok(entry) => ImageView::ready(&self.images, &entry),
            Err(e) => ImageView::error(e.to_string()),
        }
    }

    fn file_view(&self, path: &Path) -> ImageView {
        match local::thumbnail_for_file(&self.images, path) {
            Ok(entry) => ImageView::ready(&self.images, &entry),
            Err(e) => ImageView::error(e.to_string()),
        }
    }

    /// A registered asset can be previewed from Discord's CDN, which doubles
    /// as proof that the key really exists for this application.
    async fn asset_view(&self, key: Option<&str>) -> ImageView {
        let Some(key) = key.map(str::trim).filter(|k| !k.is_empty()) else {
            return ImageView::none();
        };
        let app_id = self.settings_snapshot().application_id;
        if app_id.is_empty() {
            return ImageView::unavailable(
                "Application ID를 설정하면 Discord에 등록된 이미지를 미리 볼 수 있습니다.",
            );
        }
        let assets = match self.assets.list(&self.http, &app_id, false).await {
            Ok(assets) => assets,
            Err(e) => return ImageView::unavailable(e.to_string()),
        };
        match assets.iter().find(|a| a.name.eq_ignore_ascii_case(key)) {
            None => ImageView::unavailable(format!(
                "Asset Key '{key}'가 Developer Portal에 등록돼 있지 않습니다."
            )),
            Some(asset) => {
                self.url_view(&asset_image_url(&app_id, &asset.id), false)
                    .await
            }
        }
    }

    pub fn export_dir(&self) -> std::path::PathBuf {
        effective_export_dir(&self.settings_snapshot().export_dir, &self.default_export_dir)
    }

    pub fn default_export_dir(&self) -> &Path {
        &self.default_export_dir
    }

    /// The original file behind an image setting, for tools that need the
    /// full-size pixels (cropping). A remote image is downloaded into the cache.
    pub async fn source_file(&self, spec: &ImageSpec) -> Result<std::path::PathBuf> {
        let none = || AppError::Invalid("이미지가 설정되어 있지 않습니다.".into());
        let value = |v: &Option<String>| {
            v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
        };
        match spec.source {
            ImageSource::Default => Err(none()),
            ImageSource::ExternalUrl => {
                let url = value(&spec.url).ok_or_else(none)?;
                let entry =
                    cache::fetch_with(&self.http, &self.images, &url, false, &Default::default())
                        .await?;
                Ok(self.images.path(&entry.file))
            }
            ImageSource::Uploaded => {
                local::uploaded_path(&self.paths, &value(&spec.local_path).ok_or_else(none)?)
            }
            ImageSource::LocalFile => value(&spec.local_path)
                .map(std::path::PathBuf::from)
                .ok_or_else(none),
            ImageSource::DiscordAsset => {
                let key = value(&spec.asset_key).ok_or_else(none)?;
                let app_id = self.settings_snapshot().application_id;
                let assets = self.assets.list(&self.http, &app_id, false).await?;
                let asset = assets
                    .iter()
                    .find(|a| a.name.eq_ignore_ascii_case(&key))
                    .ok_or_else(|| {
                        AppError::NotFound(format!("Asset Key '{key}'가 등록돼 있지 않습니다."))
                    })?;
                let entry = cache::fetch_with(
                    &self.http,
                    &self.images,
                    &asset_image_url(&app_id, &asset.id),
                    false,
                    &Default::default(),
                )
                .await?;
                Ok(self.images.path(&entry.file))
            }
        }
    }

    pub fn connection_state(&self) -> ConnState {
        self.discord.status().state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game() -> Game {
        Game {
            id: "elden-ring".into(),
            name: "Elden Ring".into(),
            platform: "PlayStation 5".into(),
            details: "Exploring".into(),
            state: String::new(),
            image_source: ImageSource::DiscordAsset,
            asset_key: Some("elden-ring".into()),
            ..Game::default()
        }
    }

    #[test]
    fn the_export_folder_is_the_setting_or_else_the_default() {
        let default = Path::new("/home/me/Pictures/Console Presence");
        assert_eq!(effective_export_dir("", default), default);
        assert_eq!(effective_export_dir("   ", default), default, "blank means default");
        assert_eq!(
            effective_export_dir(" /data/covers ", default),
            Path::new("/data/covers"),
            "a configured folder wins and is trimmed"
        );
    }

    #[test]
    fn blank_fields_fall_back_to_the_app_wide_defaults() {
        let settings = Settings {
            default_details: "Playing".into(),
            default_state: "On console".into(),
            ..Settings::default()
        };
        let config = presence_defaults(&game(), &settings);
        assert_eq!(config.details, "Exploring", "the game's own value wins");
        assert_eq!(config.state, "On console", "blank falls back to the default");
        assert_eq!(config.game_name, "Elden Ring");
    }

    #[test]
    fn the_platform_is_shown_in_the_hover_texts() {
        let config = presence_defaults(&game(), &Settings::default());
        assert_eq!(config.large_image_text, "Elden Ring · PlayStation 5");
        assert_eq!(config.small_image_text, "PlayStation 5");
    }

    #[test]
    fn explicit_hover_texts_and_small_image_are_kept() {
        let mut g = game();
        g.large_image_text = Some("Custom".into());
        g.small_image = Some(ImageSpec {
            source: ImageSource::DiscordAsset,
            asset_key: Some("ps5".into()),
            ..ImageSpec::default()
        });
        g.small_image_text = Some("PS5".into());
        let config = presence_defaults(&g, &Settings::default());
        assert_eq!(config.large_image_text, "Custom");
        assert_eq!(config.small_image.asset_key.as_deref(), Some("ps5"));
        assert_eq!(config.small_image_text, "PS5");
    }

    #[test]
    fn the_games_large_image_is_carried_over_unchanged() {
        let config = presence_defaults(&game(), &Settings::default());
        assert_eq!(config.large_image, game().image_spec());
        assert_eq!(config.start_timestamp, None);
    }
}
