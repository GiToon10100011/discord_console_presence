use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use sha2::{Digest, Sha256};

use super::{
    cache::{inspect, write_atomic_if_missing, CacheEntry, FetchMeta, Inspected, ImageCache},
    MAX_IMAGE_BYTES,
};
use crate::{
    error::{AppError, Result},
    storage::AppPaths,
};

/// Discord recommends 1024×1024; larger uploads are scaled down.
pub const UPLOAD_MAX_SIDE: u32 = 1024;

fn read_limited(src: &Path) -> Result<Vec<u8>> {
    let meta = fs::metadata(src)
        .map_err(|e| AppError::Image(format!("파일을 열 수 없습니다: {e}")))?;
    if !meta.is_file() {
        return Err(AppError::Image("파일이 아닙니다.".into()));
    }
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(AppError::Image(format!(
            "파일이 너무 큽니다 ({:.1}MB, 최대 {}MB).",
            meta.len() as f64 / 1_048_576.0,
            MAX_IMAGE_BYTES / 1_048_576
        )));
    }
    Ok(fs::read(src)?)
}

/// Thumbnail (and validation) for an image file on disk. Used by both local
/// source kinds; the cache key covers path, size and modification time, so
/// an edited file is picked up and an unchanged one is not re-read.
pub fn thumbnail_for_file(cache: &ImageCache, src: &Path) -> Result<CacheEntry> {
    let meta = fs::metadata(src)
        .map_err(|e| AppError::Image(format!("파일을 열 수 없습니다: {e}")))?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis());
    let source = format!("local:{}:{modified}:{}", src.display(), meta.len());
    if let Some(entry) = cache.lookup(&source) {
        return Ok(entry);
    }
    let bytes = read_limited(src)?;
    cache.store(&source, &bytes, FetchMeta::default())
}

pub struct Imported {
    /// File name inside the uploads directory (what the game stores).
    pub file_name: String,
    pub entry: CacheEntry,
}

fn shrink_for_upload(bytes: &[u8], inspected: &Inspected) -> Result<(Vec<u8>, &'static str)> {
    let (Some(width), Some(height)) = (inspected.width, inspected.height) else {
        return Ok((bytes.to_vec(), inspected.format.ext()));
    };
    if width.max(height) <= UPLOAD_MAX_SIDE {
        return Ok((bytes.to_vec(), inspected.format.ext()));
    }
    let decoded = image::load_from_memory(bytes)
        .map_err(|e| AppError::Image(format!("이미지를 읽을 수 없습니다: {e}")))?;
    let scaled = decoded.resize(
        UPLOAD_MAX_SIDE,
        UPLOAD_MAX_SIDE,
        image::imageops::FilterType::Lanczos3,
    );
    let mut png = Vec::new();
    scaled
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| AppError::Image(format!("이미지를 변환할 수 없습니다: {e}")))?;
    Ok((png, "png"))
}

/// Imports a user-chosen image: validates it, scales it down if it is larger
/// than Discord needs, and keeps a copy in the app's data directory so the
/// game keeps working if the original file moves.
pub fn import_uploaded(paths: &AppPaths, cache: &ImageCache, src: &Path) -> Result<Imported> {
    let bytes = read_limited(src)?;
    let inspected = inspect(&bytes)?;
    let (data, ext) = shrink_for_upload(&bytes, &inspected)?;

    let hash = hex::encode(Sha256::digest(&data));
    let file_name = format!("{}.{ext}", &hash[..16]);
    let dir = paths.uploads_dir();
    fs::create_dir_all(&dir)?;
    write_atomic_if_missing(&dir.join(&file_name), &data)?;

    let entry = thumbnail_for_file(cache, &dir.join(&file_name))?;
    Ok(Imported { file_name, entry })
}

/// Resolves a stored upload name to its path, refusing anything that could
/// escape the uploads directory.
pub fn uploaded_path(paths: &AppPaths, name: &str) -> Result<PathBuf> {
    let valid = !name.is_empty()
        && Path::new(name).file_name().is_some_and(|f| f == name)
        && !name.contains(['/', '\\']);
    if !valid {
        return Err(AppError::Invalid("올바르지 않은 업로드 파일 이름입니다.".into()));
    }
    Ok(paths.uploads_dir().join(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::cache::test_images::png;

    fn setup() -> (tempfile::TempDir, AppPaths, ImageCache) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        paths.ensure().unwrap();
        let cache = ImageCache::open(paths.images_dir(), u64::MAX);
        (dir, paths, cache)
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn import_copies_a_small_image_unchanged_into_the_uploads_dir() {
        let (dir, paths, cache) = setup();
        let original = png(800, 800);
        let src = write(dir.path(), "cover.png", &original);

        let imported = import_uploaded(&paths, &cache, &src).unwrap();
        let stored = paths.uploads_dir().join(&imported.file_name);
        assert_eq!(fs::read(&stored).unwrap(), original);
        assert!(imported.file_name.ends_with(".png"));
        assert_eq!(imported.entry.width, Some(800));

        fs::remove_file(&src).unwrap();
        assert!(stored.exists(), "the managed copy outlives the original");
    }

    #[test]
    fn import_scales_down_oversized_images() {
        let (dir, paths, cache) = setup();
        let src = write(dir.path(), "huge.png", &png(2400, 1200));
        let imported = import_uploaded(&paths, &cache, &src).unwrap();
        let stored = fs::read(paths.uploads_dir().join(&imported.file_name)).unwrap();
        let decoded = image::load_from_memory(&stored).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (UPLOAD_MAX_SIDE, UPLOAD_MAX_SIDE / 2));
    }

    #[test]
    fn importing_the_same_file_twice_stores_it_once() {
        let (dir, paths, cache) = setup();
        let src = write(dir.path(), "cover.png", &png(700, 700));
        let a = import_uploaded(&paths, &cache, &src).unwrap();
        let b = import_uploaded(&paths, &cache, &src).unwrap();
        assert_eq!(a.file_name, b.file_name);
        assert_eq!(fs::read_dir(paths.uploads_dir()).unwrap().count(), 1);
    }

    #[test]
    fn non_images_and_missing_files_are_rejected() {
        let (dir, paths, cache) = setup();
        let text = write(dir.path(), "notes.png", b"definitely not a png");
        assert!(import_uploaded(&paths, &cache, &text).is_err(), "the extension is irrelevant");
        assert!(import_uploaded(&paths, &cache, &dir.path().join("missing.png")).is_err());
        assert!(import_uploaded(&paths, &cache, dir.path()).is_err(), "a directory is not an image");
        assert_eq!(fs::read_dir(paths.uploads_dir()).unwrap().count(), 0);
    }

    #[test]
    fn thumbnails_are_cached_until_the_file_changes() {
        let (dir, _paths, cache) = setup();
        let src = write(dir.path(), "a.png", &png(600, 600));
        let first = thumbnail_for_file(&cache, &src).unwrap();
        let again = thumbnail_for_file(&cache, &src).unwrap();
        assert_eq!(first.key, again.key);
        assert_eq!(cache.list().len(), 1);

        fs::write(&src, png(650, 650)).unwrap();
        let changed = thumbnail_for_file(&cache, &src).unwrap();
        assert_ne!(changed.sha256, first.sha256);
        assert_eq!(changed.width, Some(650));
    }

    #[test]
    fn upload_names_cannot_escape_the_uploads_dir() {
        let (_dir, paths, _cache) = setup();
        assert!(uploaded_path(&paths, "abc123.png").is_ok());
        for bad in ["", "../secret.png", "a/b.png", "a\\b.png", "..", "/etc/passwd"] {
            assert!(uploaded_path(&paths, bad).is_err(), "{bad:?}");
        }
    }
}
