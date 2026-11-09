use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::Duration,
};

use reqwest::{header, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::MAX_IMAGE_BYTES;
use crate::{
    error::{AppError, Result},
    storage::JsonStore,
    util::{lock, now_ms, parse_http_url, public_http_url},
};

/// Largest accepted width/height. Guards against decompression bombs.
const MAX_SIDE: u32 = 8192;
const THUMB_SIDE: u32 = 384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Gif,
    Webp,
    Avif,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
            Self::Avif => "avif",
        }
    }

    /// AVIF has no pure-Rust decoder here, so it is checked by signature only.
    fn codec(self) -> Option<image::ImageFormat> {
        match self {
            Self::Png => Some(image::ImageFormat::Png),
            Self::Jpeg => Some(image::ImageFormat::Jpeg),
            Self::Gif => Some(image::ImageFormat::Gif),
            Self::Webp => Some(image::ImageFormat::WebP),
            Self::Avif => None,
        }
    }
}

/// Identifies the format from the file's magic bytes, never from its name or
/// the server's Content-Type. SVG and HTML are rejected by omission.
pub fn sniff(bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(Format::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Format::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(Format::Gif)
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(Format::Webp)
    } else if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(&bytes[8..12], b"avif" | b"avis")
    {
        Some(Format::Avif)
    } else {
        None
    }
}

#[derive(Debug)]
pub struct Inspected {
    pub format: Format,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub thumb_png: Option<Vec<u8>>,
    pub warnings: Vec<String>,
}

fn corrupt(error: image::ImageError) -> AppError {
    AppError::Image(format!("손상되었거나 읽을 수 없는 이미지입니다: {error}"))
}

pub fn quality_warnings(width: u32, height: u32) -> Vec<String> {
    let mut warnings = Vec::new();
    if width.min(height) < 512 {
        warnings.push(format!(
            "해상도가 낮습니다 ({width}×{height}). Discord는 1024×1024를 권장합니다."
        ));
    }
    if (width as f32 / height as f32 - 1.0).abs() > 0.05 {
        warnings.push(
            "정사각형이 아니어서 Discord에서 가운데 부분만 잘려 표시될 수 있습니다.".to_string(),
        );
    }
    warnings
}

/// Validates `bytes` as an image and prepares a thumbnail. Fully decodes the
/// image so a truncated or corrupt file is rejected here instead of failing
/// later in the UI.
pub fn inspect(bytes: &[u8]) -> Result<Inspected> {
    let format = sniff(bytes).ok_or_else(|| {
        AppError::Image("지원하지 않는 이미지 형식입니다 (PNG, JPEG, WebP, GIF, AVIF만 가능).".into())
    })?;
    let Some(codec) = format.codec() else {
        return Ok(Inspected {
            format,
            width: None,
            height: None,
            thumb_png: None,
            warnings: vec!["AVIF는 내용을 검증하지 않았습니다.".to_string()],
        });
    };

    let (width, height) = image::ImageReader::with_format(Cursor::new(bytes), codec)
        .into_dimensions()
        .map_err(corrupt)?;
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(AppError::Image(format!(
            "이미지 크기가 올바르지 않습니다 ({width}×{height}, 한 변 최대 {MAX_SIDE}px)."
        )));
    }
    let decoded = image::load_from_memory_with_format(bytes, codec).map_err(corrupt)?;
    let thumb = if width.max(height) > THUMB_SIDE {
        decoded.resize(THUMB_SIDE, THUMB_SIDE, image::imageops::FilterType::Triangle)
    } else {
        decoded
    };
    let mut png = Vec::new();
    thumb
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| AppError::Image(format!("썸네일을 만들 수 없습니다: {e}")))?;

    Ok(Inspected {
        format,
        width: Some(width),
        height: Some(height),
        thumb_png: Some(png),
        warnings: quality_warnings(width, height),
    })
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    pub key: String,
    /// What the entry was fetched from: an http(s) URL, or `local:...` for a
    /// thumbnail of a file on disk.
    pub url: String,
    pub file: String,
    pub thumb_file: Option<String>,
    pub thumb_bytes: u64,
    pub sha256: String,
    pub bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub format: String,
    pub fetched_at: i64,
    pub last_used_at: i64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Default)]
pub struct FetchMeta {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub entries: usize,
    pub bytes: u64,
    pub max_bytes: u64,
    pub dir: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Index {
    version: u32,
    entries: BTreeMap<String, CacheEntry>,
}

fn total_bytes(index: &Index) -> u64 {
    // Entries with identical content share files, so count each file once.
    let mut sizes: HashMap<&str, u64> = HashMap::new();
    for entry in index.entries.values() {
        sizes.insert(&entry.file, entry.bytes);
        if let Some(thumb) = &entry.thumb_file {
            sizes.insert(thumb, entry.thumb_bytes);
        }
    }
    sizes.values().sum()
}

pub fn write_atomic_if_missing(path: &Path, bytes: &[u8]) -> Result<()> {
    if fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() == bytes.len() as u64) {
        return Ok(());
    }
    let tmp = path.with_extension("tmp");
    let write = || -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    };
    write().map_err(|e| {
        let _ = fs::remove_file(&tmp);
        AppError::Storage(format!("{}: {e}", path.display()))
    })
}

/// The app-managed image cache. It is separate from game data: it can be
/// wiped at any time and is rebuilt on demand.
pub struct ImageCache {
    dir: PathBuf,
    store: JsonStore<Index>,
    index: Mutex<Index>,
    max_bytes: AtomicU64,
}

impl ImageCache {
    pub fn open(dir: PathBuf, max_bytes: u64) -> Self {
        let _ = fs::create_dir_all(&dir);
        let store = JsonStore::new(dir.join("index.json"));
        let mut index: Index = store.load();
        let before = index.entries.len();
        index.entries.retain(|_, e| dir.join(&e.file).is_file());
        let pruned = before != index.entries.len();
        let cache = Self {
            dir,
            store,
            index: Mutex::new(index),
            max_bytes: AtomicU64::new(max_bytes),
        };
        if pruned {
            cache.persist(&lock(&cache.index));
        }
        cache
    }

    pub fn set_max_bytes(&self, max_bytes: u64) {
        self.max_bytes.store(max_bytes, Ordering::Relaxed);
        let mut index = lock(&self.index);
        self.evict(&mut index, "");
        self.persist(&index);
    }

    pub fn key_for(source: &str) -> String {
        hex::encode(Sha256::digest(source.as_bytes()))
    }

    pub fn path(&self, file: &str) -> PathBuf {
        self.dir.join(file)
    }

    pub fn display_path(&self, entry: &CacheEntry) -> PathBuf {
        self.path(entry.thumb_file.as_deref().unwrap_or(&entry.file))
    }

    pub fn lookup(&self, source: &str) -> Option<CacheEntry> {
        let key = Self::key_for(source);
        let mut index = lock(&self.index);
        let entry = index.entries.get_mut(&key)?;
        if !self.dir.join(&entry.file).is_file() {
            index.entries.remove(&key);
            return None;
        }
        entry.last_used_at = now_ms();
        Some(entry.clone())
    }

    /// Marks an entry as freshly validated (after a `304 Not Modified`).
    pub fn touch(&self, source: &str) -> Option<CacheEntry> {
        let key = Self::key_for(source);
        let mut index = lock(&self.index);
        let entry = index.entries.get_mut(&key)?;
        entry.fetched_at = now_ms();
        entry.last_used_at = entry.fetched_at;
        let entry = entry.clone();
        self.persist(&index);
        Some(entry)
    }

    pub fn store(&self, source: &str, bytes: &[u8], meta: FetchMeta) -> Result<CacheEntry> {
        let inspected = inspect(bytes)?;
        let sha256 = hex::encode(Sha256::digest(bytes));
        let stem = &sha256[..16];
        let file = format!("{stem}.{}", inspected.format.ext());
        write_atomic_if_missing(&self.dir.join(&file), bytes)?;
        let (thumb_file, thumb_bytes) = match &inspected.thumb_png {
            Some(png) => {
                let name = format!("{stem}.thumb.png");
                write_atomic_if_missing(&self.dir.join(&name), png)?;
                (Some(name), png.len() as u64)
            }
            None => (None, 0),
        };

        let now = now_ms();
        let entry = CacheEntry {
            key: Self::key_for(source),
            url: source.to_string(),
            file,
            thumb_file,
            thumb_bytes,
            sha256,
            bytes: bytes.len() as u64,
            width: inspected.width,
            height: inspected.height,
            format: inspected.format.ext().to_string(),
            fetched_at: now,
            last_used_at: now,
            etag: meta.etag,
            last_modified: meta.last_modified,
            warnings: inspected.warnings,
        };

        let mut index = lock(&self.index);
        index.version = 1;
        if let Some(previous) = index.entries.insert(entry.key.clone(), entry.clone()) {
            self.remove_if_unreferenced(&index, &previous);
        }
        self.evict(&mut index, &entry.key);
        self.persist(&index);
        Ok(entry)
    }

    pub fn list(&self) -> Vec<CacheEntry> {
        let mut entries: Vec<_> = lock(&self.index).entries.values().cloned().collect();
        entries.sort_by(|a, b| b.last_used_at.cmp(&a.last_used_at));
        entries
    }

    pub fn stats(&self) -> CacheStats {
        let index = lock(&self.index);
        CacheStats {
            entries: index.entries.len(),
            bytes: total_bytes(&index),
            max_bytes: self.max_bytes.load(Ordering::Relaxed),
            dir: self.dir.to_string_lossy().into_owned(),
        }
    }

    /// Deletes every cached file. Returns how many files were removed.
    pub fn clear(&self) -> Result<usize> {
        let mut index = lock(&self.index);
        index.entries.clear();
        let mut removed = 0;
        for item in fs::read_dir(&self.dir)? {
            let path = item?.path();
            if path.is_file() && path.file_name().is_some_and(|n| n != "index.json") {
                fs::remove_file(&path)?;
                removed += 1;
            }
        }
        self.persist(&index);
        Ok(removed)
    }

    fn remove_if_unreferenced(&self, index: &Index, removed: &CacheEntry) {
        let files = [Some(removed.file.as_str()), removed.thumb_file.as_deref()];
        for file in files.into_iter().flatten() {
            let in_use = index
                .entries
                .values()
                .any(|e| e.file == file || e.thumb_file.as_deref() == Some(file));
            if !in_use {
                let _ = fs::remove_file(self.dir.join(file));
            }
        }
    }

    /// Drops least-recently-used entries until the cache fits its budget.
    /// `keep` (the entry just written) is never evicted.
    fn evict(&self, index: &mut Index, keep: &str) {
        let budget = self.max_bytes.load(Ordering::Relaxed);
        while total_bytes(index) > budget {
            let victim = index
                .entries
                .values()
                .filter(|e| e.key != keep)
                .min_by_key(|e| e.last_used_at)
                .map(|e| e.key.clone());
            let Some(key) = victim else { break };
            if let Some(entry) = index.entries.remove(&key) {
                self.remove_if_unreferenced(index, &entry);
            }
        }
    }

    fn persist(&self, index: &Index) {
        if let Err(e) = self.store.save(index) {
            log::warn!("could not save the image cache index: {e}");
        }
    }
}

#[derive(Clone, Debug)]
pub struct FetchOptions {
    /// Tests talk to 127.0.0.1; production never does.
    pub allow_private: bool,
    pub max_bytes: u64,
    pub attempts: u32,
    pub base_delay: Duration,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            allow_private: false,
            max_bytes: MAX_IMAGE_BYTES,
            attempts: 3,
            base_delay: Duration::from_millis(600),
        }
    }
}

enum Attempt {
    NotModified,
    Body(Vec<u8>, FetchMeta),
    /// Worth another try (network flap, 5xx, 429).
    Retry(AppError, Option<Duration>),
    Fail(AppError),
}

fn too_big(size: u64, max: u64) -> AppError {
    AppError::Image(format!(
        "이미지가 너무 큽니다 ({:.1}MB, 최대 {}MB).",
        size as f64 / 1_048_576.0,
        max / 1_048_576
    ))
}

fn header_text(response: &reqwest::Response, name: header::HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    let secs: u64 = header_text(response, header::RETRY_AFTER)?.trim().parse().ok()?;
    Some(Duration::from_secs(secs.min(5)))
}

async fn download_once(
    client: &reqwest::Client,
    url: &url::Url,
    existing: Option<&CacheEntry>,
    max_bytes: u64,
) -> Attempt {
    let mut request = client
        .get(url.clone())
        .header(header::ACCEPT, "image/*,*/*;q=0.5");
    if let Some(entry) = existing {
        if let Some(etag) = &entry.etag {
            request = request.header(header::IF_NONE_MATCH, etag.as_str());
        } else if let Some(modified) = &entry.last_modified {
            request = request.header(header::IF_MODIFIED_SINCE, modified.as_str());
        }
    }

    let mut response = match request.send().await {
        Ok(response) => response,
        Err(e) if e.is_redirect() || e.is_builder() => {
            return Attempt::Fail(AppError::Network(format!("이미지를 가져오지 못했습니다: {e}")))
        }
        Err(e) => {
            return Attempt::Retry(
                AppError::Network(format!("이미지를 가져오지 못했습니다: {e}")),
                None,
            )
        }
    };

    let status = response.status();
    if status == StatusCode::NOT_MODIFIED {
        return Attempt::NotModified;
    }
    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        let wait = retry_after(&response);
        return Attempt::Retry(
            AppError::Network(format!("서버가 {status} 응답을 반환했습니다.")),
            wait,
        );
    }
    if !status.is_success() {
        return Attempt::Fail(AppError::Network(format!(
            "서버가 {status} 응답을 반환했습니다."
        )));
    }
    if let Some(length) = response.content_length() {
        if length > max_bytes {
            return Attempt::Fail(too_big(length, max_bytes));
        }
    }

    let meta = FetchMeta {
        etag: header_text(&response, header::ETAG),
        last_modified: header_text(&response, header::LAST_MODIFIED),
    };
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if (body.len() + chunk.len()) as u64 > max_bytes {
                    return Attempt::Fail(too_big(body.len() as u64 + chunk.len() as u64, max_bytes));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(e) => {
                return Attempt::Retry(
                    AppError::Network(format!("다운로드가 중단되었습니다: {e}")),
                    None,
                )
            }
        }
    }
    Attempt::Body(body, meta)
}

/// Downloads `raw_url` into the cache, or returns the cached copy.
///
/// - Only public http(s) URLs are accepted.
/// - The size is capped, and the bytes must be a valid image (checked by
///   content, not by name or Content-Type).
/// - The cache key is the URL, so a changed URL fetches a fresh image while
///   the same URL is never downloaded twice; `force` revalidates with
///   `ETag`/`Last-Modified`.
/// - Transient failures (network, 5xx, 429) are retried with backoff; a
///   failed refresh leaves the previously cached copy untouched.
pub async fn fetch_with(
    client: &reqwest::Client,
    cache: &ImageCache,
    raw_url: &str,
    force: bool,
    options: &FetchOptions,
) -> Result<CacheEntry> {
    let url = if options.allow_private {
        parse_http_url(raw_url)?
    } else {
        public_http_url(raw_url)?
    };
    let source = url.as_str().to_string();
    let mut existing = cache.lookup(&source);
    if let (Some(entry), false) = (&existing, force) {
        return Ok(entry.clone());
    }

    let mut last_error = AppError::Network("이미지를 가져오지 못했습니다.".into());
    let mut wait: Option<Duration> = None;
    for attempt in 0..options.attempts.max(1) {
        if attempt > 0 {
            let backoff = options.base_delay * 3u32.saturating_pow(attempt - 1);
            tokio::time::sleep(wait.take().unwrap_or(backoff)).await;
        }
        match download_once(client, &url, existing.as_ref(), options.max_bytes).await {
            Attempt::NotModified => match cache.touch(&source) {
                Some(entry) => return Ok(entry),
                None => {
                    // The entry disappeared; download it unconditionally.
                    existing = None;
                    last_error = AppError::Network("캐시 항목을 찾을 수 없어 다시 받습니다.".into());
                }
            },
            Attempt::Body(bytes, meta) => return cache.store(&source, &bytes, meta),
            Attempt::Fail(error) => return Err(error),
            Attempt::Retry(error, retry_wait) => {
                last_error = error;
                wait = retry_wait;
            }
        }
    }
    Err(last_error)
}

pub async fn fetch(
    client: &reqwest::Client,
    cache: &ImageCache,
    raw_url: &str,
    force: bool,
) -> Result<CacheEntry> {
    fetch_with(client, cache, raw_url, force, &FetchOptions::default()).await
}

#[cfg(test)]
pub(crate) mod test_images {
    use std::io::Cursor;

    pub fn encode(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
        let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            width,
            height,
            image::Rgba([200, 60, 90, 255]),
        ));
        let img = if format == image::ImageFormat::Jpeg {
            image::DynamicImage::ImageRgb8(img.to_rgb8())
        } else {
            img
        };
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), format).unwrap();
        out
    }

    pub fn png(width: u32, height: u32) -> Vec<u8> {
        encode(width, height, image::ImageFormat::Png)
    }
}

#[cfg(test)]
mod tests {
    use super::test_images::{encode, png};
    use super::*;
    use crate::image::testserver::{Response, TestServer};

    fn cache(max_bytes: u64) -> (tempfile::TempDir, ImageCache) {
        let dir = tempfile::tempdir().unwrap();
        let cache = ImageCache::open(dir.path().join("images"), max_bytes);
        (dir, cache)
    }

    fn files_in(cache: &ImageCache) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(&cache.dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "index.json")
            .collect();
        names.sort();
        names
    }

    fn fast_options() -> FetchOptions {
        FetchOptions {
            allow_private: true,
            max_bytes: MAX_IMAGE_BYTES,
            attempts: 3,
            base_delay: Duration::from_millis(5),
        }
    }

    // ---- format detection and validation ----

    #[test]
    fn accepts_every_supported_raster_format() {
        for (format, expected) in [
            (image::ImageFormat::Png, Format::Png),
            (image::ImageFormat::Jpeg, Format::Jpeg),
            (image::ImageFormat::Gif, Format::Gif),
            (image::ImageFormat::WebP, Format::Webp),
        ] {
            let bytes = encode(64, 64, format);
            let inspected = inspect(&bytes).unwrap_or_else(|e| panic!("{format:?}: {e}"));
            assert_eq!(inspected.format, expected);
            assert_eq!((inspected.width, inspected.height), (Some(64), Some(64)));
        }
    }

    #[test]
    fn avif_is_accepted_by_signature_with_a_note() {
        let mut bytes = vec![0, 0, 0, 0x18];
        bytes.extend_from_slice(b"ftypavif");
        bytes.extend_from_slice(&[0; 16]);
        let inspected = inspect(&bytes).unwrap();
        assert_eq!(inspected.format, Format::Avif);
        assert_eq!(inspected.width, None);
        assert!(!inspected.warnings.is_empty());
    }

    #[test]
    fn non_images_are_rejected_by_content() {
        for (name, bytes) in [
            ("svg", b"<svg xmlns='http://www.w3.org/2000/svg'></svg>".to_vec()),
            ("html", b"<!doctype html><html></html>".to_vec()),
            ("json", br#"{"error":"nope"}"#.to_vec()),
            ("empty", Vec::new()),
            ("zeros", vec![0u8; 64]),
        ] {
            assert!(inspect(&bytes).is_err(), "{name} must be rejected");
        }
    }

    #[test]
    fn truncated_images_are_rejected_as_corrupt() {
        let mut bytes = png(200, 200);
        bytes.truncate(bytes.len() / 2);
        let err = inspect(&bytes).unwrap_err().to_string();
        assert!(err.contains("손상"), "{err}");
    }

    #[test]
    fn oversized_dimensions_are_rejected_before_decoding() {
        let bytes = png(MAX_SIDE + 1, 1);
        assert!(inspect(&bytes).is_err());
    }

    #[test]
    fn quality_warnings_follow_discords_recommendations() {
        let small = inspect(&png(300, 400)).unwrap();
        assert_eq!(small.warnings.len(), 2, "low resolution and not square");
        let good = inspect(&png(1024, 1024)).unwrap();
        assert!(good.warnings.is_empty());
    }

    #[test]
    fn thumbnails_are_bounded() {
        let inspected = inspect(&png(2000, 1000)).unwrap();
        let thumb = image::load_from_memory(&inspected.thumb_png.unwrap()).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (THUMB_SIDE, THUMB_SIDE / 2));
    }

    // ---- cache behaviour ----

    #[test]
    fn identical_content_from_two_urls_shares_one_file() {
        let (_dir, cache) = cache(u64::MAX);
        let bytes = png(100, 100);
        let a = cache.store("https://a.example/x.png", &bytes, FetchMeta::default()).unwrap();
        let b = cache.store("https://b.example/y.png", &bytes, FetchMeta::default()).unwrap();
        assert_eq!(a.file, b.file);
        assert_ne!(a.key, b.key);
        assert_eq!(files_in(&cache).len(), 2, "one original and one thumbnail");
        assert_eq!(cache.list().len(), 2);
    }

    #[test]
    fn replacing_an_entry_removes_the_file_it_no_longer_uses() {
        let (_dir, cache) = cache(u64::MAX);
        let old = cache.store("https://a.example/x.png", &png(100, 100), FetchMeta::default()).unwrap();
        let new = cache.store("https://a.example/x.png", &png(120, 120), FetchMeta::default()).unwrap();
        assert_ne!(old.file, new.file);
        assert!(!cache.path(&old.file).exists(), "stale file is cleaned up");
        assert!(cache.path(&new.file).exists());
    }

    #[test]
    fn a_shared_file_survives_until_its_last_user_is_gone() {
        let (_dir, cache) = cache(u64::MAX);
        let shared = png(100, 100);
        let a = cache.store("https://a.example/x.png", &shared, FetchMeta::default()).unwrap();
        cache.store("https://b.example/y.png", &shared, FetchMeta::default()).unwrap();
        cache.store("https://a.example/x.png", &png(130, 130), FetchMeta::default()).unwrap();
        assert!(cache.path(&a.file).exists(), "still used by the other URL");
    }

    #[test]
    fn least_recently_used_entries_are_evicted_over_budget() {
        let (_dir, cache) = cache(u64::MAX);
        let first = cache.store("https://x/1", &png(300, 300), FetchMeta::default()).unwrap();
        // An entry costs its original plus its thumbnail; size the budget for two.
        let per_entry = cache.stats().bytes;
        cache.set_max_bytes(per_entry * 2 + per_entry / 2);
        std::thread::sleep(Duration::from_millis(3));
        cache.store("https://x/2", &encode(301, 301, image::ImageFormat::Png), FetchMeta::default()).unwrap();
        std::thread::sleep(Duration::from_millis(3));
        // Using the first entry again makes the second the oldest.
        assert!(cache.lookup("https://x/1").is_some());
        std::thread::sleep(Duration::from_millis(3));
        cache.store("https://x/3", &encode(302, 302, image::ImageFormat::Png), FetchMeta::default()).unwrap();

        assert!(cache.lookup("https://x/1").is_some());
        assert!(cache.lookup("https://x/2").is_none(), "least recently used is evicted");
        assert!(cache.lookup("https://x/3").is_some(), "the new entry is kept");
        assert!(cache.stats().bytes <= cache.stats().max_bytes);
        assert!(cache.path(&first.file).exists());
    }

    #[test]
    fn shrinking_the_budget_evicts_immediately() {
        let (_dir, cache) = cache(u64::MAX);
        for i in 0..4u32 {
            cache.store(&format!("https://x/{i}"), &png(200 + i, 200), FetchMeta::default()).unwrap();
        }
        cache.set_max_bytes(1);
        assert!(cache.list().len() <= 1);
    }

    #[test]
    fn clear_removes_all_files_and_entries() {
        let (_dir, cache) = cache(u64::MAX);
        cache.store("https://x/1", &png(100, 100), FetchMeta::default()).unwrap();
        cache.store("https://x/2", &png(110, 110), FetchMeta::default()).unwrap();
        let removed = cache.clear().unwrap();
        assert_eq!(removed, 4);
        assert!(cache.list().is_empty());
        assert!(files_in(&cache).is_empty());
        assert_eq!(cache.stats().bytes, 0);
    }

    #[test]
    fn the_index_survives_a_restart_and_drops_entries_with_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let images = dir.path().join("images");
        let (kept, lost) = {
            let cache = ImageCache::open(images.clone(), u64::MAX);
            let kept = cache.store("https://x/kept", &png(100, 100), FetchMeta::default()).unwrap();
            let lost = cache.store("https://x/lost", &png(110, 110), FetchMeta::default()).unwrap();
            (kept, lost)
        };
        fs::remove_file(images.join(&lost.file)).unwrap();

        let reopened = ImageCache::open(images, u64::MAX);
        assert!(reopened.lookup("https://x/kept").is_some());
        assert!(reopened.lookup("https://x/lost").is_none());
        assert_eq!(reopened.lookup("https://x/kept").unwrap().sha256, kept.sha256);
    }

    #[test]
    fn rejected_images_leave_no_trace_in_the_cache() {
        let (_dir, cache) = cache(u64::MAX);
        assert!(cache.store("https://x/bad", b"not an image", FetchMeta::default()).is_err());
        assert!(files_in(&cache).is_empty());
        assert!(cache.list().is_empty());
    }

    // ---- downloading ----

    #[tokio::test]
    async fn downloads_validates_and_then_reuses_the_cached_copy() {
        let body = png(600, 600);
        let server = TestServer::start(move |_| {
            Response::ok(body.clone()).header("ETag", "\"v1\"")
        });
        let (_dir, cache) = cache(u64::MAX);
        let client = reqwest::Client::new();
        let url = format!("{}/cover.png", server.base);

        let first = fetch_with(&client, &cache, &url, false, &fast_options()).await.unwrap();
        assert_eq!(first.format, "png");
        assert_eq!(first.etag.as_deref(), Some("\"v1\""));
        assert_eq!(server.hits(), 1);

        let second = fetch_with(&client, &cache, &url, false, &fast_options()).await.unwrap();
        assert_eq!(second.sha256, first.sha256);
        assert_eq!(server.hits(), 1, "no second download for the same URL");
    }

    #[tokio::test]
    async fn a_changed_url_downloads_a_fresh_image() {
        let server = TestServer::start(|req| {
            let size = if req.target.contains("new") { 700 } else { 600 };
            Response::ok(png(size, size))
        });
        let (_dir, cache) = cache(u64::MAX);
        let client = reqwest::Client::new();
        let old = fetch_with(&client, &cache, &format!("{}/old.png", server.base), false, &fast_options()).await.unwrap();
        let new = fetch_with(&client, &cache, &format!("{}/new.png", server.base), false, &fast_options()).await.unwrap();
        assert_ne!(old.sha256, new.sha256);
        assert_eq!(new.width, Some(700));
        assert_eq!(server.hits(), 2);
    }

    #[tokio::test]
    async fn forced_refresh_revalidates_with_the_etag() {
        let body = png(600, 600);
        let server = TestServer::start(move |req| {
            if req.header("if-none-match") == Some("\"v1\"") {
                Response::status(304)
            } else {
                Response::ok(body.clone()).header("ETag", "\"v1\"")
            }
        });
        let (_dir, cache) = cache(u64::MAX);
        let client = reqwest::Client::new();
        let url = format!("{}/cover.png", server.base);
        let first = fetch_with(&client, &cache, &url, false, &fast_options()).await.unwrap();
        let again = fetch_with(&client, &cache, &url, true, &fast_options()).await.unwrap();
        assert_eq!(server.hits(), 2);
        assert_eq!(again.sha256, first.sha256);
        assert!(again.fetched_at >= first.fetched_at);
    }

    #[tokio::test]
    async fn transient_server_errors_are_retried() {
        let body = png(600, 600);
        let server = TestServer::start(move |req| match req.index {
            0 => Response::status(503),
            1 => Response::status(429).header("Retry-After", "0"),
            _ => Response::ok(body.clone()),
        });
        let (_dir, cache) = cache(u64::MAX);
        let client = reqwest::Client::new();
        let entry = fetch_with(&client, &cache, &format!("{}/c.png", server.base), false, &fast_options())
            .await
            .unwrap();
        assert_eq!(entry.format, "png");
        assert_eq!(server.hits(), 3);
    }

    #[tokio::test]
    async fn gives_up_after_the_configured_attempts() {
        let server = TestServer::start(|_| Response::status(503));
        let (_dir, cache) = cache(u64::MAX);
        let client = reqwest::Client::new();
        let err = fetch_with(&client, &cache, &format!("{}/c.png", server.base), false, &fast_options())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("503"), "{err}");
        assert_eq!(server.hits(), 3);
        assert!(cache.list().is_empty());
    }

    #[tokio::test]
    async fn client_errors_are_not_retried() {
        let server = TestServer::start(|_| Response::status(404));
        let (_dir, cache) = cache(u64::MAX);
        let client = reqwest::Client::new();
        let err = fetch_with(&client, &cache, &format!("{}/missing.png", server.base), false, &fast_options())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("404"));
        assert_eq!(server.hits(), 1);
    }

    #[tokio::test]
    async fn responses_that_are_not_images_are_rejected_and_not_cached() {
        let server = TestServer::start(|_| Response::ok("<html>blocked</html>").header("Content-Type", "image/png"));
        let (_dir, cache) = cache(u64::MAX);
        let client = reqwest::Client::new();
        let err = fetch_with(&client, &cache, &format!("{}/c.png", server.base), false, &fast_options())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("이미지 형식"), "a lying Content-Type must not help: {err}");
        assert_eq!(server.hits(), 1, "invalid content is not retried");
        assert!(cache.list().is_empty());
    }

    #[tokio::test]
    async fn oversized_downloads_are_refused() {
        let server = TestServer::start(|_| Response::ok(png(900, 900)));
        let (_dir, cache) = cache(u64::MAX);
        let client = reqwest::Client::new();
        let options = FetchOptions { max_bytes: 100, ..fast_options() };
        let err = fetch_with(&client, &cache, &format!("{}/big.png", server.base), false, &options)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("너무 큽니다"), "{err}");
        assert!(cache.list().is_empty());
    }

    #[tokio::test]
    async fn a_failed_refresh_keeps_the_previous_copy() {
        let body = png(600, 600);
        let server = TestServer::start(move |req| {
            if req.index == 0 {
                Response::ok(body.clone())
            } else {
                Response::status(500)
            }
        });
        let (_dir, cache) = cache(u64::MAX);
        let client = reqwest::Client::new();
        let url = format!("{}/c.png", server.base);
        let first = fetch_with(&client, &cache, &url, false, &fast_options()).await.unwrap();
        assert!(fetch_with(&client, &cache, &url, true, &fast_options()).await.is_err());
        let kept = cache.lookup(&Url::parse(&url).unwrap().to_string()).expect("still cached");
        assert_eq!(kept.sha256, first.sha256);
        assert!(cache.path(&kept.file).exists());
    }

    #[tokio::test]
    async fn production_fetch_refuses_private_and_non_http_urls() {
        let (_dir, cache) = cache(u64::MAX);
        let client = reqwest::Client::new();
        for bad in ["http://127.0.0.1:1/a.png", "file:///etc/passwd", "http://192.168.0.1/a.png", "gopher://x/a"] {
            let err = fetch(&client, &cache, bad, false).await.unwrap_err();
            assert!(matches!(err, AppError::Invalid(_)), "{bad}: {err}");
        }
    }

    use url::Url;
}
