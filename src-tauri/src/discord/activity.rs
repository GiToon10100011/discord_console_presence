use discord_rich_presence::activity::{Activity, ActivityType, Assets, Timestamps};
use serde::Serialize;

use crate::{
    image::assets::AssetLookup,
    session::PresenceConfig,
    storage::{clamp_text, ImageSource, ImageSpec, Settings, TEXT_MAX},
    util::public_http_url,
};

/// Discord rejects `details`, `state` and hover texts shorter than 2 characters.
const TEXT_MIN: usize = 2;

/// Converts the app's millisecond timestamps to the value sent over RPC.
///
/// Discord's docs disagree on the unit: the Social SDK guide says Unix
/// seconds, while the gateway Activity table (and the crate's doc comment)
/// say milliseconds. Seconds is what IPC clients have historically sent;
/// `examples/live_check.rs` verifies the behaviour against a real client.
pub fn rpc_timestamp(ms: i64) -> i64 {
    ms / 1000
}

/// What is sent to Discord, already validated and sanitised.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityPayload {
    pub name: Option<String>,
    pub details: Option<String>,
    pub state: Option<String>,
    pub large_image: Option<String>,
    pub large_text: Option<String>,
    pub small_image: Option<String>,
    pub small_text: Option<String>,
    /// Start of the elapsed timer, Unix ms.
    pub start_ms: Option<i64>,
}

impl ActivityPayload {
    pub fn to_rpc(&self) -> Activity<'_> {
        let mut activity = Activity::new().activity_type(ActivityType::Playing);
        if let Some(name) = &self.name {
            activity = activity.name(name.as_str());
        }
        if let Some(details) = &self.details {
            activity = activity.details(details.as_str());
        }
        if let Some(state) = &self.state {
            activity = activity.state(state.as_str());
        }

        let mut assets = Assets::new();
        let mut has_assets = false;
        if let Some(image) = &self.large_image {
            assets = assets.large_image(image.as_str());
            if let Some(text) = &self.large_text {
                assets = assets.large_text(text.as_str());
            }
            has_assets = true;
        }
        if let Some(image) = &self.small_image {
            assets = assets.small_image(image.as_str());
            if let Some(text) = &self.small_text {
                assets = assets.small_text(text.as_str());
            }
            has_assets = true;
        }
        if has_assets {
            activity = activity.assets(assets);
        }

        if let Some(start) = self.start_ms {
            activity = activity.timestamps(Timestamps::new().start(rpc_timestamp(start)));
        }
        activity
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImageKind {
    /// A Developer Portal asset key.
    Asset,
    /// An external URL fetched by Discord's media proxy.
    Url,
    /// The configured fallback replaced an unusable or missing image.
    Fallback,
    #[default]
    None,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImageOutcome {
    pub kind: ImageKind,
    pub value: Option<String>,
}

/// How the presence was assembled. Discord never reports whether an image
/// actually rendered, so this is the most the app can say: what it sent, and
/// why it replaced anything.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PresenceReport {
    pub warnings: Vec<String>,
    pub large: ImageOutcome,
    pub small: ImageOutcome,
}

struct Resolved {
    value: String,
    kind: ImageKind,
}

fn text(value: &str) -> Option<String> {
    let t = clamp_text(value, TEXT_MAX);
    (t.chars().count() >= TEXT_MIN).then_some(t)
}

fn resolve_asset(key: &str, catalog: &AssetLookup, app_id: &str) -> Result<Resolved, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("Asset Key가 비어 있습니다.".into());
    }
    // The Developer Portal lowercases asset keys when they are uploaded.
    let lower = key.to_lowercase();
    match catalog {
        AssetLookup::Known(names) if !names.iter().any(|n| *n == lower) => Err(format!(
            "Asset Key '{key}'가 Discord Developer Portal(앱 {app_id})에 등록돼 있지 않습니다."
        )),
        _ => Ok(Resolved {
            value: lower,
            kind: ImageKind::Asset,
        }),
    }
}

fn resolve_url(url: &str) -> Result<Resolved, String> {
    public_http_url(url).map_err(|e| e.to_string())?;
    Ok(Resolved {
        value: url.trim().to_string(),
        kind: ImageKind::Url,
    })
}

/// `Ok(None)` means "no image requested", which is not an error.
fn resolve_spec(
    spec: &ImageSpec,
    catalog: &AssetLookup,
    app_id: &str,
) -> Result<Option<Resolved>, String> {
    match spec.source {
        ImageSource::Default => Ok(None),
        ImageSource::DiscordAsset => {
            resolve_asset(spec.asset_key.as_deref().unwrap_or(""), catalog, app_id).map(Some)
        }
        ImageSource::ExternalUrl => resolve_url(spec.url.as_deref().unwrap_or("")).map(Some),
        ImageSource::Uploaded | ImageSource::LocalFile => {
            match spec.asset_key.as_deref().filter(|k| !k.trim().is_empty()) {
                Some(key) => resolve_asset(key, catalog, app_id).map(Some),
                None => Err(
                    "로컬 이미지는 Discord가 직접 가져올 수 없습니다. Developer Portal에 업로드한 뒤 \
                     그 Asset Key를 입력하거나, 공개 URL을 사용하세요."
                        .into(),
                ),
            }
        }
    }
}

fn fallback_image(
    settings: &Settings,
    catalog: &AssetLookup,
    warnings: &mut Vec<String>,
) -> Option<Resolved> {
    let app_id = settings.application_id.as_str();
    let key = settings.fallback_asset_key.trim();
    if !key.is_empty() {
        match resolve_asset(key, catalog, app_id) {
            Ok(r) => {
                return Some(Resolved {
                    kind: ImageKind::Fallback,
                    ..r
                })
            }
            Err(reason) => warnings.push(format!("기본 이미지: {reason}")),
        }
    }
    let url = settings.fallback_image_url.trim();
    if !url.is_empty() {
        match resolve_url(url) {
            Ok(r) => {
                return Some(Resolved {
                    kind: ImageKind::Fallback,
                    ..r
                })
            }
            Err(reason) => warnings.push(format!("기본 이미지: {reason}")),
        }
    }
    None
}

/// Turns a session's presence config into the payload sent to Discord.
///
/// A problem with an image never blocks the presence: the large image falls
/// back to the configured default and the reason is reported as a warning.
pub fn build_activity(
    config: &PresenceConfig,
    start_ms: i64,
    settings: &Settings,
    catalog: &AssetLookup,
) -> (ActivityPayload, PresenceReport) {
    let app_id = settings.application_id.as_str();
    let mut warnings = Vec::new();

    let large = match resolve_spec(&config.large_image, catalog, app_id) {
        Ok(resolved) => resolved,
        Err(reason) => {
            warnings.push(format!("큰 이미지: {reason}"));
            None
        }
    }
    .or_else(|| fallback_image(settings, catalog, &mut warnings));

    let small = match resolve_spec(&config.small_image, catalog, app_id) {
        Ok(resolved) => resolved,
        Err(reason) => {
            warnings.push(format!("작은 이미지: {reason}"));
            None
        }
    };

    let large_text = text(&config.large_image_text).or_else(|| text(&config.game_name));
    let payload = ActivityPayload {
        name: settings
            .override_activity_name
            .then(|| clamp_text(&config.game_name, TEXT_MAX))
            .filter(|n| !n.is_empty()),
        details: text(&config.details),
        state: text(&config.state),
        large_text: large.as_ref().and(large_text),
        small_text: small.as_ref().and_then(|_| text(&config.small_image_text)),
        large_image: large.as_ref().map(|r| r.value.clone()),
        small_image: small.as_ref().map(|r| r.value.clone()),
        start_ms: Some(start_ms),
    };
    let report = PresenceReport {
        warnings,
        large: ImageOutcome {
            kind: large.as_ref().map_or(ImageKind::None, |r| r.kind),
            value: payload.large_image.clone(),
        },
        small: ImageOutcome {
            kind: small.as_ref().map_or(ImageKind::None, |r| r.kind),
            value: payload.small_image.clone(),
        },
    };
    (payload, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: i64 = 1_762_646_400_000;

    fn settings() -> Settings {
        Settings {
            application_id: "123456789012345678".into(),
            ..Settings::default()
        }
    }

    fn known(names: &[&str]) -> AssetLookup {
        AssetLookup::Known(names.iter().map(|s| s.to_string()).collect())
    }

    fn asset(key: &str) -> ImageSpec {
        ImageSpec {
            source: ImageSource::DiscordAsset,
            asset_key: Some(key.into()),
            ..ImageSpec::default()
        }
    }

    fn url(u: &str) -> ImageSpec {
        ImageSpec {
            source: ImageSource::ExternalUrl,
            url: Some(u.into()),
            ..ImageSpec::default()
        }
    }

    fn config(large: ImageSpec) -> PresenceConfig {
        PresenceConfig {
            game_name: "Pokémon Legends: Z-A".into(),
            details: "Story Progress".into(),
            state: "Lumiose City".into(),
            large_image: large,
            large_image_text: "Pokémon Legends: Z-A — Nintendo Switch 2".into(),
            ..PresenceConfig::default()
        }
    }

    #[test]
    fn registered_asset_is_sent_lowercased_without_warnings() {
        let (payload, report) = build_activity(
            &config(asset("Pokemon-ZA")),
            START,
            &settings(),
            &known(&["pokemon-za", "other"]),
        );
        assert_eq!(payload.large_image.as_deref(), Some("pokemon-za"));
        assert_eq!(report.large.kind, ImageKind::Asset);
        assert!(report.warnings.is_empty());
        assert_eq!(payload.name.as_deref(), Some("Pokémon Legends: Z-A"));
        assert_eq!(payload.details.as_deref(), Some("Story Progress"));
        assert_eq!(payload.state.as_deref(), Some("Lumiose City"));
        assert_eq!(payload.start_ms, Some(START));
    }

    #[test]
    fn unregistered_asset_key_warns_and_uses_the_fallback_asset() {
        let mut s = settings();
        s.fallback_asset_key = "console-default".into();
        let (payload, report) = build_activity(
            &config(asset("not-uploaded")),
            START,
            &s,
            &known(&["console-default"]),
        );
        assert_eq!(payload.large_image.as_deref(), Some("console-default"));
        assert_eq!(report.large.kind, ImageKind::Fallback);
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("not-uploaded"));
        assert!(report.warnings[0].contains("123456789012345678"));
    }

    #[test]
    fn unregistered_asset_without_fallback_sends_no_image() {
        let (payload, report) = build_activity(
            &config(asset("missing")),
            START,
            &settings(),
            &known(&[]),
        );
        assert_eq!(payload.large_image, None);
        assert_eq!(payload.large_text, None, "hover text needs an image");
        assert_eq!(report.large.kind, ImageKind::None);
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn when_the_asset_list_is_unavailable_the_key_is_passed_through() {
        let (payload, report) = build_activity(
            &config(asset("pokemon-za")),
            START,
            &settings(),
            &AssetLookup::Unknown,
        );
        assert_eq!(payload.large_image.as_deref(), Some("pokemon-za"));
        assert!(report.warnings.is_empty(), "no false alarm when offline");
    }

    #[test]
    fn external_url_is_sent_as_is() {
        let u = "https://upload.wikimedia.org/wikipedia/en/b/b9/Elden_Ring_Box_art.jpg";
        let (payload, report) = build_activity(&config(url(u)), START, &settings(), &AssetLookup::Unknown);
        assert_eq!(payload.large_image.as_deref(), Some(u));
        assert_eq!(report.large.kind, ImageKind::Url);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn private_or_non_http_urls_are_never_sent() {
        for bad in ["http://localhost/a.png", "file:///etc/passwd", "http://10.0.0.1/a.png", "nonsense"] {
            let (payload, report) =
                build_activity(&config(url(bad)), START, &settings(), &AssetLookup::Unknown);
            assert_eq!(payload.large_image, None, "{bad}");
            assert_eq!(report.warnings.len(), 1, "{bad}");
        }
    }

    #[test]
    fn local_image_without_a_registered_key_explains_the_limitation() {
        let spec = ImageSpec {
            source: ImageSource::LocalFile,
            local_path: Some("/Users/me/cover.png".into()),
            ..ImageSpec::default()
        };
        let (payload, report) = build_activity(&config(spec), START, &settings(), &known(&[]));
        assert_eq!(payload.large_image, None);
        assert!(report.warnings[0].contains("Developer Portal"));
    }

    #[test]
    fn local_image_uploaded_to_the_portal_uses_its_asset_key() {
        let spec = ImageSpec {
            source: ImageSource::Uploaded,
            local_path: Some("abc.png".into()),
            asset_key: Some("my-cover".into()),
            ..ImageSpec::default()
        };
        let (payload, report) =
            build_activity(&config(spec), START, &settings(), &known(&["my-cover"]));
        assert_eq!(payload.large_image.as_deref(), Some("my-cover"));
        assert_eq!(report.large.kind, ImageKind::Asset);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn default_source_uses_the_fallback_silently_or_sends_nothing() {
        let mut s = settings();
        let (none, report) = build_activity(&config(ImageSpec::default()), START, &s, &known(&[]));
        assert_eq!(none.large_image, None);
        assert!(report.warnings.is_empty());

        s.fallback_image_url = "https://example.com/default.png".into();
        let (fb, report) = build_activity(&config(ImageSpec::default()), START, &s, &known(&[]));
        assert_eq!(fb.large_image.as_deref(), Some("https://example.com/default.png"));
        assert_eq!(report.large.kind, ImageKind::Fallback);
    }

    #[test]
    fn small_image_failures_warn_but_never_fall_back() {
        let mut cfg = config(asset("pokemon-za"));
        cfg.small_image = asset("ps5-icon");
        cfg.small_image_text = "PlayStation 5".into();
        let (payload, report) =
            build_activity(&cfg, START, &settings(), &known(&["pokemon-za"]));
        assert_eq!(payload.small_image, None);
        assert_eq!(payload.small_text, None);
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].starts_with("작은 이미지"));

        let (ok, _) = build_activity(&cfg, START, &settings(), &known(&["pokemon-za", "ps5-icon"]));
        assert_eq!(ok.small_image.as_deref(), Some("ps5-icon"));
        assert_eq!(ok.small_text.as_deref(), Some("PlayStation 5"));
    }

    #[test]
    fn text_fields_follow_discords_length_rules() {
        let mut cfg = config(ImageSpec::default());
        cfg.details = "x".into();
        cfg.state = "y".repeat(500);
        let (payload, _) = build_activity(&cfg, START, &settings(), &AssetLookup::Unknown);
        assert_eq!(payload.details, None, "single-character text is dropped");
        assert_eq!(payload.state.as_ref().map(|s| s.chars().count()), Some(TEXT_MAX));
    }

    #[test]
    fn empty_hover_text_defaults_to_the_game_name() {
        let mut cfg = config(asset("pokemon-za"));
        cfg.large_image_text.clear();
        let (payload, _) = build_activity(&cfg, START, &settings(), &known(&["pokemon-za"]));
        assert_eq!(payload.large_text.as_deref(), Some("Pokémon Legends: Z-A"));
    }

    #[test]
    fn activity_name_override_can_be_turned_off() {
        let mut s = settings();
        s.override_activity_name = false;
        let (payload, _) = build_activity(&config(ImageSpec::default()), START, &s, &AssetLookup::Unknown);
        assert_eq!(payload.name, None);
    }

    #[test]
    fn rpc_json_has_the_documented_shape() {
        let (payload, _) = build_activity(
            &config(asset("pokemon-za")),
            START,
            &settings(),
            &known(&["pokemon-za"]),
        );
        let json = serde_json::to_value(payload.to_rpc()).unwrap();
        assert_eq!(json["type"], 0, "Playing");
        assert_eq!(json["name"], "Pokémon Legends: Z-A");
        assert_eq!(json["details"], "Story Progress");
        assert_eq!(json["state"], "Lumiose City");
        assert_eq!(json["assets"]["large_image"], "pokemon-za");
        assert_eq!(json["assets"]["large_text"], "Pokémon Legends: Z-A — Nintendo Switch 2");
        assert_eq!(json["timestamps"]["start"], START / 1000);
        assert!(json.get("buttons").is_none() && json.get("party").is_none());
    }

    #[test]
    fn payload_without_images_has_no_assets_object() {
        let (payload, _) = build_activity(
            &config(ImageSpec::default()),
            START,
            &settings(),
            &AssetLookup::Unknown,
        );
        let json = serde_json::to_value(payload.to_rpc()).unwrap();
        assert!(json.get("assets").is_none());
    }
}
