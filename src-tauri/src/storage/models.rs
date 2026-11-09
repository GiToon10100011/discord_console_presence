use serde::{Deserialize, Serialize};

use crate::{
    error::{AppError, Result},
    util::{non_empty, now_ms, parse_http_url, slugify},
};

const DEFAULT_GAMES_JSON: &str = include_str!("default_games.json");

/// Discord limits `details`, `state` and hover texts to 128 characters.
pub const TEXT_MAX: usize = 128;
const NAME_MAX: usize = 100;
const PLATFORM_MAX: usize = 40;
const URL_MAX: usize = 2048;
const KEY_MAX: usize = 256;

pub fn clamp_text(value: &str, max: usize) -> String {
    value.trim().chars().take(max).collect()
}

/// Where an image comes from. The sources are deliberately distinct: only
/// `DiscordAsset` and `ExternalUrl` can be shown by Discord directly.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImageSource {
    /// Asset registered in the Discord Developer Portal (referenced by key).
    DiscordAsset,
    /// Public https image fetched by Discord's media proxy.
    ExternalUrl,
    /// File the user imported; a managed copy lives in the app data dir.
    Uploaded,
    /// File referenced in place on the user's disk.
    LocalFile,
    /// No image: the configured fallback is used.
    #[default]
    Default,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ImageSpec {
    pub source: ImageSource,
    /// Asset key. For `Uploaded`/`LocalFile` it names the Developer Portal
    /// asset the file was uploaded as, which is what Discord will display.
    pub asset_key: Option<String>,
    pub url: Option<String>,
    /// `Uploaded`: file name inside the uploads dir. `LocalFile`: absolute path.
    pub local_path: Option<String>,
}

impl ImageSpec {
    pub fn is_none(&self) -> bool {
        self.source == ImageSource::Default
    }

    pub fn normalize(&mut self) {
        self.asset_key = non_empty(self.asset_key.take());
        self.url = non_empty(self.url.take());
        self.local_path = non_empty(self.local_path.take());
    }

    /// Strict check used when saving a game: a chosen source must be usable.
    pub fn validate(&self) -> Result<()> {
        match self.source {
            ImageSource::Default => {}
            ImageSource::DiscordAsset => {
                let key = self.asset_key.as_deref().unwrap_or("");
                if key.is_empty() {
                    return Err(AppError::Invalid("Discord Asset Key를 입력하세요.".into()));
                }
                if key.chars().count() > KEY_MAX {
                    return Err(AppError::Invalid("Asset Key가 너무 깁니다.".into()));
                }
            }
            ImageSource::ExternalUrl => {
                let url = self.url.as_deref().unwrap_or("");
                if url.is_empty() {
                    return Err(AppError::Invalid("이미지 URL을 입력하세요.".into()));
                }
                if url.chars().count() > URL_MAX {
                    return Err(AppError::Invalid("이미지 URL이 너무 깁니다.".into()));
                }
                parse_http_url(url)?;
            }
            ImageSource::Uploaded | ImageSource::LocalFile => {
                if self.local_path.as_deref().unwrap_or("").is_empty() {
                    return Err(AppError::Invalid("이미지 파일을 선택하세요.".into()));
                }
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Game {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub details: String,
    pub state: String,
    pub image_source: ImageSource,
    pub asset_key: Option<String>,
    pub image_url: Option<String>,
    pub local_image_path: Option<String>,
    pub large_image_text: Option<String>,
    pub small_image: Option<ImageSpec>,
    pub small_image_text: Option<String>,
    pub favorite: bool,
    pub builtin: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Game {
    pub fn image_spec(&self) -> ImageSpec {
        ImageSpec {
            source: self.image_source,
            asset_key: self.asset_key.clone(),
            url: self.image_url.clone(),
            local_path: self.local_image_path.clone(),
        }
    }

    pub fn normalize(&mut self) {
        self.id = self.id.trim().to_string();
        self.name = clamp_text(&self.name, NAME_MAX);
        self.platform = clamp_text(&self.platform, PLATFORM_MAX);
        self.details = clamp_text(&self.details, TEXT_MAX);
        self.state = clamp_text(&self.state, TEXT_MAX);
        self.asset_key = non_empty(self.asset_key.take());
        self.image_url = non_empty(self.image_url.take());
        self.local_image_path = non_empty(self.local_image_path.take());
        self.large_image_text = non_empty(self.large_image_text.take())
            .map(|t| clamp_text(&t, TEXT_MAX));
        self.small_image_text = non_empty(self.small_image_text.take())
            .map(|t| clamp_text(&t, TEXT_MAX));
        if let Some(small) = self.small_image.as_mut() {
            small.normalize();
        }
        if self.small_image.as_ref().is_some_and(ImageSpec::is_none) {
            self.small_image = None;
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            return Err(AppError::Invalid("게임 이름을 입력하세요.".into()));
        }
        if self.platform.is_empty() {
            return Err(AppError::Invalid("플랫폼을 선택하세요.".into()));
        }
        self.image_spec().validate()?;
        if let Some(small) = &self.small_image {
            small
                .validate()
                .map_err(|e| AppError::Invalid(format!("작은 이미지: {e}")))?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Library {
    pub version: u32,
    pub games: Vec<Game>,
    /// Ids of bundled games that were already offered, so a game the user
    /// deleted is not resurrected on the next launch.
    pub seeded_ids: Vec<String>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            version: 1,
            games: Vec::new(),
            seeded_ids: Vec::new(),
        }
    }
}

pub fn default_games() -> Vec<Game> {
    serde_json::from_str(DEFAULT_GAMES_JSON).expect("bundled default_games.json is valid")
}

impl Library {
    /// Adds bundled games that were never offered before. Returns whether anything changed.
    pub fn seed_defaults(&mut self) -> bool {
        let now = now_ms();
        let mut changed = false;
        for mut game in default_games() {
            if self.seeded_ids.contains(&game.id) {
                continue;
            }
            self.seeded_ids.push(game.id.clone());
            changed = true;
            if self.games.iter().any(|g| g.id == game.id) {
                continue;
            }
            game.builtin = true;
            game.created_at = now;
            game.updated_at = now;
            self.games.push(game);
        }
        changed
    }

    pub fn find(&self, id: &str) -> Option<&Game> {
        self.games.iter().find(|g| g.id == id)
    }

    fn unique_id(&self, base: &str) -> String {
        let slug = slugify(base);
        if self.find(&slug).is_none() {
            return slug;
        }
        (2..)
            .map(|n| format!("{slug}-{n}"))
            .find(|candidate| self.find(candidate).is_none())
            .expect("unbounded range always yields a free id")
    }

    pub fn add(&mut self, mut game: Game) -> Result<Game> {
        game.normalize();
        game.validate()?;
        let now = now_ms();
        let base = if game.id.is_empty() {
            game.name.clone()
        } else {
            game.id.clone()
        };
        game.id = self.unique_id(&base);
        game.builtin = false;
        game.created_at = now;
        game.updated_at = now;
        self.games.push(game.clone());
        Ok(game)
    }

    pub fn update(&mut self, mut game: Game) -> Result<Game> {
        game.normalize();
        game.validate()?;
        let slot = self
            .games
            .iter_mut()
            .find(|g| g.id == game.id)
            .ok_or_else(|| AppError::NotFound(format!("게임을 찾을 수 없습니다: {}", game.id)))?;
        game.builtin = slot.builtin;
        game.created_at = slot.created_at;
        game.updated_at = now_ms();
        *slot = game.clone();
        Ok(game)
    }

    pub fn remove(&mut self, id: &str) -> Result<Game> {
        let index = self
            .games
            .iter()
            .position(|g| g.id == id)
            .ok_or_else(|| AppError::NotFound(format!("게임을 찾을 수 없습니다: {id}")))?;
        Ok(self.games.remove(index))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Discord Application (client) ID used for the RPC handshake.
    pub application_id: String,
    pub auto_connect: bool,
    pub launch_at_login: bool,
    /// Resume a session that was still running when the app last closed.
    pub restore_last_session: bool,
    /// Sessions are ended automatically after this many hours. 0 disables it.
    pub auto_clear_hours: u32,
    pub default_details: String,
    pub default_state: String,
    pub default_image_source: ImageSource,
    /// Image shown for games without a usable image (asset key, preferred).
    pub fallback_asset_key: String,
    /// Image shown for games without a usable image (public URL).
    pub fallback_image_url: String,
    /// Send the game name as the activity name ("Playing <game>") instead of
    /// the registered application name.
    pub override_activity_name: bool,
    pub image_cache_max_mb: u32,
    /// Public https link to the live stream (Twitch, YouTube, CHZZK, SOOP, ...).
    /// Used while a session is marked "live".
    pub stream_url: String,
    /// Text on the state line while live, after a red dot.
    pub stream_label: String,
    /// Label of the button that opens the stream (Discord allows 32 characters).
    pub stream_button_label: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            application_id: String::new(),
            auto_connect: true,
            launch_at_login: false,
            restore_last_session: true,
            auto_clear_hours: 12,
            default_details: "Playing".to_string(),
            default_state: String::new(),
            default_image_source: ImageSource::DiscordAsset,
            fallback_asset_key: String::new(),
            fallback_image_url: String::new(),
            override_activity_name: true,
            image_cache_max_mb: 200,
            stream_url: String::new(),
            stream_label: "방송 중".to_string(),
            stream_button_label: "방송 보러 가기".to_string(),
        }
    }
}

impl Settings {
    pub fn sanitize(&mut self) -> Result<()> {
        self.application_id = self.application_id.trim().to_string();
        let id = &self.application_id;
        if !id.is_empty()
            && !((10..=22).contains(&id.len()) && id.chars().all(|c| c.is_ascii_digit()))
        {
            return Err(AppError::Invalid(
                "Application ID는 10~22자리 숫자여야 합니다.".into(),
            ));
        }
        self.default_details = clamp_text(&self.default_details, TEXT_MAX);
        self.default_state = clamp_text(&self.default_state, TEXT_MAX);
        self.fallback_asset_key = self.fallback_asset_key.trim().to_string();
        self.fallback_image_url = self.fallback_image_url.trim().to_string();
        if !self.fallback_image_url.is_empty() {
            parse_http_url(&self.fallback_image_url)?;
        }
        self.stream_url = self.stream_url.trim().to_string();
        if !self.stream_url.is_empty() {
            let url = crate::util::public_http_url(&self.stream_url)?;
            if url.scheme() != "https" || self.stream_url.chars().count() > 512 {
                return Err(AppError::Invalid(
                    "방송 주소는 512자 이하의 https 주소여야 합니다.".into(),
                ));
            }
        }
        self.stream_label = clamp_text(&self.stream_label, 32);
        if self.stream_label.is_empty() {
            self.stream_label = "방송 중".to_string();
        }
        self.stream_button_label = clamp_text(&self.stream_button_label, 32);
        if self.stream_button_label.is_empty() {
            self.stream_button_label = "방송 보러 가기".to_string();
        }
        self.auto_clear_hours = self.auto_clear_hours.min(168);
        self.image_cache_max_mb = self.image_cache_max_mb.clamp(16, 2048);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(name: &str) -> Game {
        Game {
            name: name.into(),
            platform: "Nintendo Switch".into(),
            ..Game::default()
        }
    }

    #[test]
    fn bundled_games_parse_and_have_unique_ids() {
        let games = default_games();
        assert_eq!(games.len(), 8);
        let mut ids: Vec<_> = games.iter().map(|g| g.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 8, "ids must be unique");
        for g in &games {
            assert_eq!(g.image_source, ImageSource::DiscordAsset, "{}", g.id);
            assert!(g.asset_key.as_deref().is_some_and(|k| !k.is_empty()), "{}", g.id);
        }
    }

    #[test]
    fn seeding_is_idempotent_and_does_not_resurrect_deleted_games() {
        let mut lib = Library::default();
        assert!(lib.seed_defaults());
        assert_eq!(lib.games.len(), 8);
        assert!(!lib.seed_defaults(), "second seed is a no-op");

        lib.remove("elden-ring").unwrap();
        assert!(!lib.seed_defaults());
        assert!(lib.find("elden-ring").is_none());
    }

    #[test]
    fn add_generates_unique_ids() {
        let mut lib = Library::default();
        let a = lib.add(game("Hades")).unwrap();
        let b = lib.add(game("Hades")).unwrap();
        assert_eq!(a.id, "hades");
        assert_eq!(b.id, "hades-2");
        assert!(!a.builtin);
    }

    #[test]
    fn add_rejects_missing_name_and_unusable_image() {
        let mut lib = Library::default();
        assert!(lib.add(game("   ")).is_err());

        let mut g = game("Bad Asset");
        g.image_source = ImageSource::DiscordAsset;
        assert!(lib.add(g).is_err(), "asset source without a key");

        let mut g = game("Bad Url");
        g.image_source = ImageSource::ExternalUrl;
        g.image_url = Some("file:///etc/passwd".into());
        assert!(lib.add(g).is_err(), "non-http url");
    }

    #[test]
    fn update_keeps_identity_fields() {
        let mut lib = Library::default();
        lib.seed_defaults();
        let mut edited = lib.find("elden-ring").unwrap().clone();
        let created = edited.created_at;
        edited.state = "Caelid".into();
        edited.builtin = false; // callers must not be able to change this
        edited.created_at = 1;
        let saved = lib.update(edited).unwrap();
        assert_eq!(saved.state, "Caelid");
        assert!(saved.builtin);
        assert_eq!(saved.created_at, created);
    }

    #[test]
    fn update_and_remove_report_unknown_ids() {
        let mut lib = Library::default();
        let mut g = game("Ghost");
        g.id = "ghost".into();
        assert!(matches!(lib.update(g), Err(AppError::NotFound(_))));
        assert!(matches!(lib.remove("ghost"), Err(AppError::NotFound(_))));
    }

    #[test]
    fn normalize_trims_and_clamps() {
        let mut g = game("  Zelda  ");
        g.details = "x".repeat(300);
        g.asset_key = Some("   ".into());
        g.small_image = Some(ImageSpec::default());
        g.normalize();
        assert_eq!(g.name, "Zelda");
        assert_eq!(g.details.chars().count(), TEXT_MAX);
        assert_eq!(g.asset_key, None);
        assert_eq!(g.small_image, None, "an empty small image is dropped");
    }

    #[test]
    fn settings_validate_application_id() {
        let mut s = Settings::default();
        assert!(s.sanitize().is_ok(), "empty id is allowed (not configured yet)");
        s.application_id = " 123456789012345678 ".into();
        assert!(s.sanitize().is_ok());
        assert_eq!(s.application_id, "123456789012345678");
        s.application_id = "abc".into();
        assert!(s.sanitize().is_err());
        s.application_id = "12345".into();
        assert!(s.sanitize().is_err());
    }

    #[test]
    fn settings_clamp_numeric_ranges_and_reject_bad_fallback_url() {
        let mut s = Settings {
            image_cache_max_mb: 1,
            auto_clear_hours: 9999,
            ..Settings::default()
        };
        s.sanitize().unwrap();
        assert_eq!(s.image_cache_max_mb, 16);
        assert_eq!(s.auto_clear_hours, 168);

        s.fallback_image_url = "javascript:alert(1)".into();
        assert!(s.sanitize().is_err());
    }

    #[test]
    fn stream_settings_are_validated_and_defaulted() {
        let mut s = Settings::default();
        assert_eq!(s.stream_label, "방송 중");
        s.stream_url = " https://www.twitch.tv/me ".into();
        s.stream_label = "   ".into();
        s.stream_button_label = "x".repeat(80);
        s.sanitize().unwrap();
        assert_eq!(s.stream_url, "https://www.twitch.tv/me");
        assert_eq!(s.stream_label, "방송 중", "a blank label falls back to the default");
        assert_eq!(s.stream_button_label.chars().count(), 32, "Discord allows 32 characters");

        for bad in [
            "http://www.twitch.tv/me",
            "https://localhost/x",
            "https://192.168.0.2/x",
            "javascript:alert(1)",
            "not a url",
        ] {
            let mut s = Settings {
                stream_url: bad.into(),
                ..Settings::default()
            };
            assert!(s.sanitize().is_err(), "{bad} must be rejected");
        }
        let mut long = Settings {
            stream_url: format!("https://example.com/{}", "a".repeat(600)),
            ..Settings::default()
        };
        assert!(long.sanitize().is_err(), "a button URL is limited to 512 characters");
    }

    #[test]
    fn old_files_with_missing_fields_still_load() {
        let s: Settings = serde_json::from_str(r#"{"applicationId":"123456789012345678"}"#).unwrap();
        assert!(s.auto_connect, "missing fields take their defaults");
        let g: Game = serde_json::from_str(r#"{"id":"a","name":"A","platform":"PC"}"#).unwrap();
        assert_eq!(g.image_source, ImageSource::Default);
    }
}
