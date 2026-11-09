pub mod assets;
pub mod cache;
pub mod cover;
pub mod local;

use std::{path::PathBuf, time::Duration};

use serde::Serialize;
use url::Url;

use crate::{
    error::{AppError, Result},
    util::is_public_host,
};
use cache::{CacheEntry, ImageCache};

/// Identifies the app to the servers it talks to (Wikimedia requires a
/// descriptive User-Agent with a way to reach the maintainer).
pub const USER_AGENT: &str =
    "ConsolePresence/0.1 (+https://github.com/GiToon10100011/discord_game_adapter)";

/// Upper bound for any image the app stores (download or local file).
pub const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;

/// A redirect may only lead to another public http(s) address.
pub fn redirect_target_ok(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https") && is_public_host(url)
}

pub fn http_client() -> Result<reqwest::Client> {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            attempt.error("리다이렉트가 너무 많습니다.")
        } else if !redirect_target_ok(attempt.url()) {
            attempt.error("허용되지 않는 리다이렉트 대상입니다.")
        } else {
            attempt.follow()
        }
    });
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirect(policy)
        .build()
        .map_err(|e| AppError::Network(format!("HTTP 클라이언트를 만들 수 없습니다: {e}")))
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImageStatus {
    Ready,
    /// No image is configured.
    None,
    /// An image is configured but cannot be shown right now (not registered,
    /// offline, no application id, ...). `message` says why.
    Unavailable,
    Error,
}

/// What the UI needs to show an image. Paths always point into the app's
/// cache or uploads directory, the only places the asset protocol serves.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImageView {
    pub status: ImageStatus,
    pub display_path: Option<String>,
    pub full_path: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bytes: Option<u64>,
    pub format: Option<String>,
    pub warnings: Vec<String>,
    pub message: Option<String>,
}

impl ImageView {
    fn blank(status: ImageStatus, message: Option<String>) -> Self {
        Self {
            status,
            display_path: None,
            full_path: None,
            width: None,
            height: None,
            bytes: None,
            format: None,
            warnings: Vec::new(),
            message,
        }
    }

    pub fn none() -> Self {
        Self::blank(ImageStatus::None, None)
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::blank(ImageStatus::Unavailable, Some(message.into()))
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::blank(ImageStatus::Error, Some(message.into()))
    }

    pub fn ready(cache: &ImageCache, entry: &CacheEntry) -> Self {
        let path = |p: PathBuf| p.to_string_lossy().into_owned();
        Self {
            status: ImageStatus::Ready,
            display_path: Some(path(cache.display_path(entry))),
            full_path: Some(path(cache.path(&entry.file))),
            width: entry.width,
            height: entry.height,
            bytes: Some(entry.bytes),
            format: Some(entry.format.clone()),
            warnings: entry.warnings.clone(),
            message: None,
        }
    }
}

/// A tiny HTTP server for tests, so downloads, retries and API clients are
/// exercised without the network.
#[cfg(test)]
pub(crate) mod testserver {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        thread,
        time::Duration,
    };

    pub struct Response {
        pub status: u16,
        pub headers: Vec<(String, String)>,
        pub body: Vec<u8>,
    }

    impl Response {
        pub fn ok(body: impl Into<Vec<u8>>) -> Self {
            Self {
                status: 200,
                headers: Vec::new(),
                body: body.into(),
            }
        }

        pub fn status(status: u16) -> Self {
            Self {
                status,
                headers: Vec::new(),
                body: Vec::new(),
            }
        }

        pub fn header(mut self, name: &str, value: &str) -> Self {
            self.headers.push((name.to_string(), value.to_string()));
            self
        }
    }

    /// What the handler sees of a request.
    pub struct Request {
        /// Zero-based index of this request.
        pub index: usize,
        /// Path and query string.
        pub target: String,
        /// Lowercased header names.
        pub headers: Vec<(String, String)>,
    }

    impl Request {
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        }
    }

    pub struct TestServer {
        pub base: String,
        pub requests: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
    }

    impl TestServer {
        pub fn start(handler: impl Fn(&Request) -> Response + Send + Sync + 'static) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let handler = Arc::new(handler);
            let (log, flag) = (requests.clone(), stop.clone());
            thread::spawn(move || {
                while !flag.load(Ordering::SeqCst) {
                    let Ok((mut stream, _)) = listener.accept() else {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    };
                    let (handler, log) = (handler.clone(), log.clone());
                    thread::spawn(move || {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut raw = Vec::new();
                        let mut chunk = [0u8; 1024];
                        while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
                            match stream.read(&mut chunk) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => raw.extend_from_slice(&chunk[..n]),
                            }
                        }
                        let text = String::from_utf8_lossy(&raw).to_string();
                        let mut lines = text.lines();
                        let target = lines
                            .next()
                            .and_then(|l| l.split_whitespace().nth(1))
                            .unwrap_or("/")
                            .to_string();
                        let headers = lines
                            .take_while(|l| !l.is_empty())
                            .filter_map(|l| l.split_once(':'))
                            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                            .collect();
                        let index = {
                            let mut log = log.lock().unwrap();
                            log.push(target.clone());
                            log.len() - 1
                        };
                        let response = handler(&Request {
                            index,
                            target,
                            headers,
                        });
                        let mut head = format!(
                            "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                            response.status,
                            response.body.len()
                        );
                        for (k, v) in &response.headers {
                            head.push_str(&format!("{k}: {v}\r\n"));
                        }
                        head.push_str("\r\n");
                        let _ = stream.write_all(head.as_bytes());
                        let _ = stream.write_all(&response.body);
                    });
                }
            });
            Self {
                base,
                requests,
                stop,
            }
        }

        pub fn hits(&self) -> usize {
            self.requests.lock().unwrap().len()
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_may_only_lead_to_public_http_hosts() {
        let ok = |s: &str| redirect_target_ok(&Url::parse(s).unwrap());
        assert!(ok("https://cdn.example.com/a.png"));
        assert!(!ok("http://127.0.0.1/a.png"));
        assert!(!ok("http://169.254.169.254/latest"));
        assert!(!ok("ftp://example.com/a.png"));
    }

    #[test]
    fn client_builds() {
        http_client().unwrap();
    }
}
