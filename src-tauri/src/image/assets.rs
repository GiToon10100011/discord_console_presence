use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use serde::Serialize;
use serde_json::Value;

use crate::{
    error::{AppError, Result},
    util::{lock, now_ms},
};

pub const API_BASE: &str = "https://discord.com/api/v10";
pub const CDN_BASE: &str = "https://cdn.discordapp.com";
const TTL: Duration = Duration::from_secs(60);

/// What is known about the assets registered for the application.
#[derive(Clone, Debug, PartialEq)]
pub enum AssetLookup {
    /// The list could not be fetched (offline, rate limited, no id yet).
    /// Keys are then sent as typed instead of being second-guessed.
    Unknown,
    /// Registered asset names, lowercased.
    Known(Vec<String>),
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppAsset {
    pub id: String,
    pub name: String,
    pub kind: Option<u8>,
}

struct Cached {
    app_id: String,
    at: Instant,
    assets: Vec<AppAsset>,
}

/// The Rich Presence assets registered for an application, read from
/// Discord's public endpoint `GET /oauth2/applications/{id}/assets`. It needs
/// no token, which keeps the app within "no user credentials".
pub struct AssetCatalog {
    base: String,
    cached: Mutex<Option<Cached>>,
}

impl Default for AssetCatalog {
    fn default() -> Self {
        Self::with_base(API_BASE)
    }
}

pub fn parse_assets(body: &str) -> Result<Vec<AppAsset>> {
    let value: Value = serde_json::from_str(body)
        .map_err(|_| AppError::Network("Discord가 예상하지 못한 응답을 반환했습니다.".into()))?;
    let items = value
        .as_array()
        .ok_or_else(|| AppError::Network("Discord가 예상하지 못한 응답을 반환했습니다.".into()))?;
    Ok(items
        .iter()
        .filter_map(|item| {
            let id = match &item["id"] {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => return None,
            };
            // The id ends up in a CDN URL: digits only.
            if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            Some(AppAsset {
                id,
                name: item["name"].as_str()?.to_string(),
                kind: item["type"].as_u64().and_then(|t| u8::try_from(t).ok()),
            })
        })
        .collect())
}

/// Discord serves the asset list through a CDN that keeps responses for up to
/// 10 minutes (`cache-control: s-maxage=600`), so a plain request does not
/// show an asset that was uploaded a moment ago. Observed: the same request
/// returned 1 asset, and with an extra query string 2. A unique query string
/// always reaches the origin.
fn cache_buster() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!("{}-{}", now_ms(), COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Where Discord serves a registered asset's image, so it can be previewed.
pub fn asset_image_url(app_id: &str, asset_id: &str) -> String {
    format!("{CDN_BASE}/app-assets/{app_id}/{asset_id}.png")
}

impl AssetCatalog {
    pub fn with_base(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            cached: Mutex::new(None),
        }
    }

    pub fn invalidate(&self) {
        *lock(&self.cached) = None;
    }

    pub async fn list(
        &self,
        client: &reqwest::Client,
        app_id: &str,
        force: bool,
    ) -> Result<Vec<AppAsset>> {
        let app_id = app_id.trim();
        if app_id.is_empty() {
            return Err(AppError::Invalid("Application ID를 먼저 설정하세요.".into()));
        }
        if !force {
            if let Some(cached) = lock(&self.cached).as_ref() {
                if cached.app_id == app_id && cached.at.elapsed() < TTL {
                    return Ok(cached.assets.clone());
                }
            }
        }

        let url = format!(
            "{}/oauth2/applications/{app_id}/assets?cb={}",
            self.base,
            cache_buster()
        );
        let response = client
            .get(&url)
            .header(reqwest::header::CACHE_CONTROL, "no-cache")
            .send()
            .await
            .map_err(|e| AppError::Network(format!("Discord에 연결하지 못했습니다: {e}")))?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(AppError::NotFound(
                "Discord에서 이 Application ID를 찾을 수 없습니다.".into(),
            ));
        }
        if !status.is_success() {
            return Err(AppError::Network(format!(
                "Discord API가 {status} 응답을 반환했습니다."
            )));
        }
        let body = response
            .text()
            .await
            .map_err(|e| AppError::Network(format!("Discord 응답을 읽지 못했습니다: {e}")))?;
        let assets = parse_assets(&body)?;
        *lock(&self.cached) = Some(Cached {
            app_id: app_id.to_string(),
            at: Instant::now(),
            assets: assets.clone(),
        });
        Ok(assets)
    }

    /// Like `list`, but a failure degrades to `Unknown` instead of an error.
    pub async fn lookup(&self, client: &reqwest::Client, app_id: &str) -> AssetLookup {
        if app_id.trim().is_empty() {
            return AssetLookup::Unknown;
        }
        match self.list(client, app_id, false).await {
            Ok(assets) => {
                AssetLookup::Known(assets.into_iter().map(|a| a.name.to_lowercase()).collect())
            }
            Err(e) => {
                log::debug!("asset list unavailable: {e}");
                AssetLookup::Unknown
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::testserver::{Response, TestServer};

    const SAMPLE: &str = r#"[
        {"id":"111111111111111111","type":1,"name":"pokemon-za"},
        {"id":"222222222222222222","type":1,"name":"Elden-Ring"}
    ]"#;

    #[test]
    fn parses_the_asset_list() {
        let assets = parse_assets(SAMPLE).unwrap();
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].id, "111111111111111111");
        assert_eq!(assets[0].name, "pokemon-za");
        assert_eq!(assets[0].kind, Some(1));
    }

    #[test]
    fn ignores_malformed_items_and_non_numeric_ids() {
        let assets = parse_assets(
            r#"[{"id":"../../etc","name":"x"},{"name":"no-id"},{"id":"123"},{"id":"456","name":"ok"}]"#,
        )
        .unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].name, "ok");
    }

    #[test]
    fn rejects_responses_that_are_not_a_list() {
        assert!(parse_assets(r#"{"message":"Unknown Application","code":10002}"#).is_err());
        assert!(parse_assets("<html>").is_err());
    }

    #[test]
    fn builds_the_documented_cdn_url() {
        assert_eq!(
            asset_image_url("123", "456"),
            "https://cdn.discordapp.com/app-assets/123/456.png"
        );
    }

    #[tokio::test]
    async fn lists_and_caches_assets() {
        let server = TestServer::start(|req| {
            assert!(req
                .target
                .starts_with("/oauth2/applications/123456789012345678/assets"));
            Response::ok(SAMPLE)
        });
        let catalog = AssetCatalog::with_base(&server.base);
        let client = reqwest::Client::new();

        let first = catalog.list(&client, "123456789012345678", false).await.unwrap();
        assert_eq!(first.len(), 2);
        catalog.list(&client, "123456789012345678", false).await.unwrap();
        assert_eq!(server.hits(), 1, "second call is served from the cache");
        catalog.list(&client, "123456789012345678", true).await.unwrap();
        assert_eq!(server.hits(), 2, "force bypasses the cache");
    }

    #[tokio::test]
    async fn every_request_bypasses_the_discord_cdn_cache() {
        // Discord's CDN answers with a list that can be 10 minutes old, which
        // made a freshly uploaded asset look unregistered.
        let server = TestServer::start(|req| {
            assert_eq!(req.header("cache-control"), Some("no-cache"));
            Response::ok(SAMPLE)
        });
        let catalog = AssetCatalog::with_base(&server.base);
        let client = reqwest::Client::new();
        catalog.list(&client, "123456789012345678", true).await.unwrap();
        catalog.list(&client, "123456789012345678", true).await.unwrap();

        let targets = server.requests.lock().unwrap().clone();
        assert_eq!(targets.len(), 2);
        for target in &targets {
            assert!(target.contains("?cb="), "no cache-busting query in {target}");
        }
        assert_ne!(targets[0], targets[1], "each request must have its own query string");
    }

    #[tokio::test]
    async fn lookup_lowercases_names_for_matching() {
        let server = TestServer::start(|_| Response::ok(SAMPLE));
        let catalog = AssetCatalog::with_base(&server.base);
        let client = reqwest::Client::new();
        let lookup = catalog.lookup(&client, "123456789012345678").await;
        assert_eq!(
            lookup,
            AssetLookup::Known(vec!["pokemon-za".into(), "elden-ring".into()])
        );
    }

    #[tokio::test]
    async fn an_unknown_application_is_reported_clearly() {
        let server = TestServer::start(|_| {
            Response {
                status: 404,
                headers: vec![],
                body: br#"{"message":"Unknown Application","code":10002}"#.to_vec(),
            }
        });
        let catalog = AssetCatalog::with_base(&server.base);
        let client = reqwest::Client::new();
        let err = catalog.list(&client, "123456789012345678", false).await.unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        assert_eq!(
            catalog.lookup(&client, "123456789012345678").await,
            AssetLookup::Unknown,
            "a failed lookup must not block the presence"
        );
    }

    #[tokio::test]
    async fn an_empty_application_id_is_rejected_without_a_request() {
        let server = TestServer::start(|_| Response::ok("[]"));
        let catalog = AssetCatalog::with_base(&server.base);
        let client = reqwest::Client::new();
        assert!(catalog.list(&client, "  ", false).await.is_err());
        assert_eq!(catalog.lookup(&client, "").await, AssetLookup::Unknown);
        assert_eq!(server.hits(), 0);
    }

    #[tokio::test]
    async fn the_cache_is_per_application() {
        let server = TestServer::start(|_| Response::ok(SAMPLE));
        let catalog = AssetCatalog::with_base(&server.base);
        let client = reqwest::Client::new();
        catalog.list(&client, "123456789012345678", false).await.unwrap();
        catalog.list(&client, "987654321098765432", false).await.unwrap();
        assert_eq!(server.hits(), 2);
    }
}
