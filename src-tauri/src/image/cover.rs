use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;
use url::Url;

use super::cache::quality_warnings;
use crate::{
    error::{AppError, Result},
    util::lock,
};

pub const API: &str = "https://en.wikipedia.org/w/api.php";
const ATTEMPTS: u32 = 3;
const RESULT_LIMIT: &str = "6";

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoverCandidate {
    pub title: String,
    pub page_url: String,
    pub file_title: String,
    pub image_url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub mime: Option<String>,
    pub warnings: Vec<String>,
    pub source: String,
}

fn clean_file_name(value: &str) -> Option<String> {
    let mut v = value.trim().to_string();
    while let (Some(start), Some(end)) = (v.find("<!--"), v.find("-->")) {
        if start >= end {
            break;
        }
        v.replace_range(start..end + 3, "");
    }
    let v = v.trim();
    let v = v.strip_prefix("[[").unwrap_or(v);
    let v = ["File:", "Image:", "file:", "image:"]
        .iter()
        .find_map(|prefix| v.strip_prefix(prefix))
        .unwrap_or(v);
    let v = v.split(['|', ']']).next().unwrap_or("").trim();
    let lower = v.to_ascii_lowercase();
    ["jpg", "jpeg", "png", "webp", "gif"]
        .iter()
        .any(|ext| lower.ends_with(&format!(".{ext}")))
        .then(|| v.to_string())
}

/// Finds the cover file named in an article's infobox. `cover` wins over the
/// generic `image` parameter.
pub fn extract_cover_file(wikitext: &str) -> Option<String> {
    let mut fallback = None;
    for line in wikitext.lines() {
        let Some(rest) = line.trim().strip_prefix('|') else {
            continue;
        };
        let Some((key, value)) = rest.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        if key != "cover" && key != "image" {
            continue;
        }
        let Some(file) = clean_file_name(value) else {
            continue;
        };
        if key == "cover" {
            return Some(file);
        }
        fallback.get_or_insert(file);
    }
    fallback
}

fn normalize_title(title: &str) -> String {
    title.replace('_', " ").trim().to_lowercase()
}

fn page_url(title: &str) -> String {
    let mut url = Url::parse("https://en.wikipedia.org/wiki/").expect("static url");
    if let Ok(mut segments) = url.path_segments_mut() {
        segments.pop_if_empty().push(&title.replace(' ', "_"));
    }
    url.to_string()
}

/// Looks up game cover art on English Wikipedia, which needs no API key.
///
/// This is a best-effort helper, not a catalogue: article search is fuzzy
/// (a query can match a film or a sequel), and the covers are small. So it
/// returns candidates for the user to pick from instead of choosing one.
pub struct CoverFinder {
    api: String,
    spacing: Duration,
    retry_base: Duration,
    next: Mutex<Instant>,
}

impl Default for CoverFinder {
    fn default() -> Self {
        Self::new(API, Duration::from_millis(700), Duration::from_millis(1500))
    }
}

impl CoverFinder {
    pub fn new(api: &str, spacing: Duration, retry_base: Duration) -> Self {
        Self {
            api: api.to_string(),
            spacing,
            retry_base,
            next: Mutex::new(Instant::now()),
        }
    }

    /// Wikimedia answers bursts with 429, so requests are spaced out.
    async fn wait_turn(&self) {
        let delay = {
            let mut next = lock(&self.next);
            let now = Instant::now();
            let at = (*next).max(now);
            *next = at + self.spacing;
            at - now
        };
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
    }

    async fn get_json(&self, client: &reqwest::Client, params: &[(&str, &str)]) -> Result<Value> {
        let url = Url::parse_with_params(&self.api, params)
            .map_err(|e| AppError::Network(format!("잘못된 요청입니다: {e}")))?;
        for attempt in 0..ATTEMPTS {
            self.wait_turn().await;
            let backoff = self.retry_base * 2u32.saturating_pow(attempt);
            let response = match client.get(url.clone()).send().await {
                Ok(response) => response,
                Err(_) => {
                    tokio::time::sleep(backoff).await;
                    continue;
                }
            };
            let status = response.status();
            if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                let wait = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .map(|secs| Duration::from_secs(secs.min(5)))
                    .unwrap_or(backoff);
                tokio::time::sleep(wait).await;
                continue;
            }
            if !status.is_success() {
                return Err(AppError::Network(format!(
                    "Wikipedia가 {status} 응답을 반환했습니다."
                )));
            }
            // A throttled or broken reply can be plain text; retry those too.
            match response.text().await.ok().and_then(|t| serde_json::from_str(&t).ok()) {
                Some(value) => return Ok(value),
                None => tokio::time::sleep(backoff).await,
            }
        }
        Err(AppError::Network(
            "Wikipedia가 요청을 제한했거나 응답하지 않습니다. 잠시 후 다시 시도하세요.".into(),
        ))
    }

    pub async fn search(&self, client: &reqwest::Client, query: &str) -> Result<Vec<CoverCandidate>> {
        let query = query.trim();
        if query.chars().count() < 2 {
            return Err(AppError::Invalid("검색어를 2자 이상 입력하세요.".into()));
        }

        // One request returns the infobox of every search hit...
        let search = format!("{query} video game");
        let first = self
            .get_json(
                client,
                &[
                    ("action", "query"),
                    ("format", "json"),
                    ("formatversion", "2"),
                    ("generator", "search"),
                    ("gsrsearch", &search),
                    ("gsrlimit", RESULT_LIMIT),
                    ("prop", "revisions"),
                    ("rvprop", "content"),
                    ("rvslots", "main"),
                    ("rvsection", "0"),
                ],
            )
            .await?;

        let mut hits: Vec<(i64, String, String)> = Vec::new();
        for page in first["query"]["pages"].as_array().into_iter().flatten() {
            let Some(title) = page["title"].as_str() else {
                continue;
            };
            let content = page["revisions"][0]["slots"]["main"]["content"]
                .as_str()
                .unwrap_or("");
            if let Some(file) = extract_cover_file(content) {
                let rank = page["index"].as_i64().unwrap_or(i64::MAX);
                hits.push((rank, title.to_string(), file));
            }
        }
        hits.sort_by_key(|(rank, ..)| *rank);
        if hits.is_empty() {
            return Ok(Vec::new());
        }

        // ...and one more resolves all cover files to image URLs.
        let titles = hits
            .iter()
            .map(|(_, _, file)| format!("File:{file}"))
            .collect::<Vec<_>>()
            .join("|");
        let second = self
            .get_json(
                client,
                &[
                    ("action", "query"),
                    ("format", "json"),
                    ("formatversion", "2"),
                    ("titles", &titles),
                    ("prop", "imageinfo"),
                    ("iiprop", "url|size|mime"),
                ],
            )
            .await?;

        let mut infos: HashMap<String, &Value> = HashMap::new();
        for page in second["query"]["pages"].as_array().into_iter().flatten() {
            if let (Some(title), info) = (page["title"].as_str(), &page["imageinfo"][0]) {
                if info["url"].is_string() {
                    infos.insert(normalize_title(title), info);
                }
            }
        }

        let mut candidates = Vec::new();
        for (_, title, file) in hits {
            let file_title = format!("File:{file}");
            let Some(info) = infos.get(&normalize_title(&file_title)) else {
                continue;
            };
            let Some(mut image_url) = info["url"].as_str().and_then(|u| Url::parse(u).ok()) else {
                continue;
            };
            image_url.set_query(None);
            let width = info["width"].as_u64().and_then(|v| u32::try_from(v).ok());
            let height = info["height"].as_u64().and_then(|v| u32::try_from(v).ok());
            candidates.push(CoverCandidate {
                page_url: page_url(&title),
                title,
                file_title,
                image_url: image_url.to_string(),
                width,
                height,
                mime: info["mime"].as_str().map(str::to_string),
                warnings: match (width, height) {
                    (Some(w), Some(h)) if w > 0 && h > 0 => quality_warnings(w, h),
                    _ => Vec::new(),
                },
                source: "wikipedia".to_string(),
            });
        }
        Ok(candidates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::testserver::{Response, TestServer};

    #[test]
    fn extracts_the_cover_from_common_infobox_shapes() {
        let plain = "{{Infobox video game\n| title = Elden Ring\n| image = Elden Ring Box art.jpg\n| caption = x\n}}";
        assert_eq!(extract_cover_file(plain).as_deref(), Some("Elden Ring Box art.jpg"));

        let linked = "{{Infobox video game\n| cover = [[File:Elden Ring Nightreign cover.png|250px]]\n}}";
        assert_eq!(
            extract_cover_file(linked).as_deref(),
            Some("Elden Ring Nightreign cover.png")
        );

        let commented = "{{Infobox\n| image = Persona 5 cover art.jpg <!-- keep -->\n}}";
        assert_eq!(extract_cover_file(commented).as_deref(), Some("Persona 5 cover art.jpg"));
    }

    #[test]
    fn cover_parameter_wins_over_image() {
        let both = "{{Infobox\n| image = Logo.png\n| cover = Real cover.jpg\n}}";
        assert_eq!(extract_cover_file(both).as_deref(), Some("Real cover.jpg"));
    }

    #[test]
    fn non_image_files_and_missing_fields_yield_nothing() {
        assert_eq!(extract_cover_file("{{Infobox film\n| name = X\n}}"), None);
        assert_eq!(extract_cover_file("| image = Trailer.ogv"), None);
        assert_eq!(extract_cover_file("| image = "), None);
        assert_eq!(extract_cover_file(""), None);
    }

    #[test]
    fn page_urls_are_percent_encoded() {
        assert_eq!(
            page_url("Pokémon Legends: Z-A"),
            "https://en.wikipedia.org/wiki/Pok%C3%A9mon_Legends:_Z-A"
        );
    }

    const SEARCH: &str = r#"{"batchcomplete":true,"query":{"pages":[
        {"pageid":1,"title":"Elden Ring (film)","index":1,"revisions":[{"slots":{"main":{"content":"{{Infobox film\n| name = Elden Ring\n}}"}}}]},
        {"pageid":3,"title":"Elden Ring Nightreign","index":3,"revisions":[{"slots":{"main":{"content":"{{Infobox video game\n| cover = [[File:Elden Ring Nightreign cover.png|250px]]\n}}"}}}]},
        {"pageid":2,"title":"Elden Ring","index":2,"revisions":[{"slots":{"main":{"content":"{{Infobox video game\n| title = Elden Ring\n| image = Elden Ring Box art.jpg\n}}"}}}]}
    ]}}"#;

    const FILES: &str = r#"{"query":{"pages":[
        {"title":"File:Elden Ring Box art.jpg","imageinfo":[{"url":"https://upload.wikimedia.org/wikipedia/en/b/b9/Elden_Ring_Box_art.jpg?utm_source=en.wikipedia.org&utm_campaign=imageinfo","width":273,"height":364,"mime":"image/jpeg"}]},
        {"title":"File:Elden Ring Nightreign cover.png","imageinfo":[{"url":"https://upload.wikimedia.org/wikipedia/en/1/1a/Nightreign.png","width":1000,"height":1000,"mime":"image/png"}]}
    ]}}"#;

    fn finder(base: &str) -> CoverFinder {
        CoverFinder::new(base, Duration::ZERO, Duration::from_millis(5))
    }

    fn api(base: &str) -> String {
        format!("{base}/w/api.php")
    }

    #[tokio::test]
    async fn returns_ranked_candidates_with_clean_urls_and_quality_warnings() {
        let server = TestServer::start(|req| {
            if req.target.contains("generator=search") {
                Response::ok(SEARCH)
            } else {
                Response::ok(FILES)
            }
        });
        let found = finder(&api(&server.base))
            .search(&reqwest::Client::new(), "Elden Ring")
            .await
            .unwrap();

        assert_eq!(server.hits(), 2, "one search request and one file lookup");
        let titles: Vec<_> = found.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["Elden Ring", "Elden Ring Nightreign"], "film without a cover is skipped, order follows search rank");

        let first = &found[0];
        assert_eq!(
            first.image_url,
            "https://upload.wikimedia.org/wikipedia/en/b/b9/Elden_Ring_Box_art.jpg",
            "tracking query is stripped"
        );
        assert_eq!((first.width, first.height), (Some(273), Some(364)));
        assert_eq!(first.warnings.len(), 2, "low resolution and not square");
        assert!(found[1].warnings.is_empty());
        assert_eq!(first.source, "wikipedia");
    }

    #[tokio::test]
    async fn no_hits_means_no_second_request() {
        let server = TestServer::start(|_| Response::ok(r#"{"batchcomplete":true}"#));
        let found = finder(&api(&server.base))
            .search(&reqwest::Client::new(), "zzzzzz")
            .await
            .unwrap();
        assert!(found.is_empty());
        assert_eq!(server.hits(), 1);
    }

    #[tokio::test]
    async fn rate_limiting_is_retried_then_succeeds() {
        let server = TestServer::start(|req| match req.index {
            0 => Response::ok("You are making too many requests").header("Content-Type", "text/plain"),
            1 => Response::status(429).header("Retry-After", "0"),
            _ if req.target.contains("generator=search") => Response::ok(SEARCH),
            _ => Response::ok(FILES),
        });
        let found = finder(&api(&server.base))
            .search(&reqwest::Client::new(), "Elden Ring")
            .await
            .unwrap();
        assert_eq!(found.len(), 2);
    }

    #[tokio::test]
    async fn persistent_throttling_gives_a_helpful_error() {
        let server = TestServer::start(|_| Response::status(429).header("Retry-After", "0"));
        let err = finder(&api(&server.base))
            .search(&reqwest::Client::new(), "Elden Ring")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("잠시 후"), "{err}");
        assert_eq!(server.hits(), ATTEMPTS as usize);
    }

    #[tokio::test]
    async fn requests_are_spaced_apart() {
        let server = TestServer::start(|_| Response::ok(r#"{"batchcomplete":true}"#));
        let finder = CoverFinder::new(&api(&server.base), Duration::from_millis(60), Duration::from_millis(5));
        let client = reqwest::Client::new();
        let start = Instant::now();
        finder.search(&client, "aa").await.unwrap();
        finder.search(&client, "bb").await.unwrap();
        assert!(start.elapsed() >= Duration::from_millis(55), "second request waited for its turn");
    }

    #[tokio::test]
    async fn very_short_queries_are_rejected_without_a_request() {
        let server = TestServer::start(|_| Response::ok("{}"));
        let err = finder(&api(&server.base))
            .search(&reqwest::Client::new(), " a ")
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Invalid(_)));
        assert_eq!(server.hits(), 0);
    }
}
