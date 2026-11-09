pub mod models;

pub use models::*;

use std::{
    fs,
    io::Write,
    marker::PhantomData,
    path::{Path, PathBuf},
};

use serde::{de::DeserializeOwned, Serialize};

use crate::{
    error::{AppError, Result},
    util::now_ms,
};

/// Locations used by the app. `data_dir` holds user data that must survive a
/// cache wipe; `cache_dir` holds everything that can be rebuilt.
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl AppPaths {
    pub fn settings_file(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    pub fn library_file(&self) -> PathBuf {
        self.data_dir.join("library.json")
    }

    pub fn session_file(&self) -> PathBuf {
        self.data_dir.join("session.json")
    }

    pub fn uploads_dir(&self) -> PathBuf {
        self.data_dir.join("uploads")
    }

    pub fn images_dir(&self) -> PathBuf {
        self.cache_dir.join("images")
    }

    /// Fallback export folder for when the system has no Pictures folder.
    /// Earlier versions exported here, so it stays openable.
    pub fn exports_dir(&self) -> PathBuf {
        self.data_dir.join("exports")
    }

    pub fn ensure(&self) -> Result<()> {
        for dir in [&self.data_dir, &self.uploads_dir(), &self.images_dir()] {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// A single JSON document on disk, written atomically (temp file + rename) so
/// a crash mid-write can never leave a truncated file behind.
pub struct JsonStore<T> {
    path: PathBuf,
    _marker: PhantomData<fn() -> T>,
}

impl<T: Serialize + DeserializeOwned + Default> JsonStore<T> {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            _marker: PhantomData,
        }
    }

    /// Loads the document. A missing file yields the default value; an
    /// unreadable one is moved aside (`*.corrupt-<ts>`) so it is not lost.
    pub fn load(&self) -> T {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return T::default(),
            Err(err) => {
                log::warn!("failed to read {}: {err}", self.path.display());
                return T::default();
            }
        };
        match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(err) => {
                log::error!("{} is corrupt ({err}); moving it aside", self.path.display());
                quarantine(&self.path);
                T::default()
            }
        }
    }

    pub fn save(&self, value: &T) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(value)?;
        let write = || -> std::io::Result<()> {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&tmp, &self.path)
        };
        write().map_err(|err| {
            let _ = fs::remove_file(&tmp);
            AppError::Storage(format!("{}: {err}", self.path.display()))
        })
    }
}

fn quarantine(path: &Path) {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".corrupt-{}", now_ms() / 1000));
    if let Err(err) = fs::rename(path, PathBuf::from(name)) {
        log::error!("could not quarantine {}: {err}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
    struct Doc {
        n: u32,
    }

    #[test]
    fn missing_file_yields_default() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonStore::<Doc>::new(dir.path().join("doc.json"));
        assert_eq!(store.load(), Doc::default());
    }

    #[test]
    fn save_then_load_round_trips_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonStore::<Doc>::new(dir.path().join("doc.json"));
        store.save(&Doc { n: 7 }).unwrap();
        assert_eq!(store.load(), Doc { n: 7 });
        assert!(!dir.path().join("doc.json.tmp").exists());
    }

    #[test]
    fn corrupt_file_is_moved_aside_not_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.json");
        fs::write(&path, "{ not json").unwrap();
        let store = JsonStore::<Doc>::new(path.clone());
        assert_eq!(store.load(), Doc::default());
        assert!(!path.exists());
        let moved = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(moved, "corrupt file should be preserved for inspection");
    }
}
