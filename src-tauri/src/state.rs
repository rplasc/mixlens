//! Shared app state and helpers used by both commands and AI tools.

use crate::db;
use crate::settings::Settings;
use mixlens_core::{Audio, Targets};
use rusqlite::Connection;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Keeps the most recently used decoded files in memory (two is enough for a comparison).
pub struct AudioCache {
    cap: usize,
    entries: VecDeque<(i64, Arc<Audio>)>,
}

impl AudioCache {
    pub fn new(cap: usize) -> Self {
        Self { cap, entries: VecDeque::new() }
    }
    pub fn get(&mut self, id: i64) -> Option<Arc<Audio>> {
        let pos = self.entries.iter().position(|(k, _)| *k == id)?;
        let item = self.entries.remove(pos)?;
        let a = item.1.clone();
        self.entries.push_front(item);
        Some(a)
    }
    pub fn put(&mut self, id: i64, a: Arc<Audio>) {
        self.entries.retain(|(k, _)| *k != id);
        self.entries.push_front((id, a));
        self.entries.truncate(self.cap);
    }
    pub fn remove(&mut self, id: i64) {
        self.entries.retain(|(k, _)| *k != id);
    }
}

pub struct Inner {
    pub db: Mutex<Connection>,
    pub cache: Mutex<AudioCache>,
    pub http: reqwest::Client,
}

pub type Shared = Arc<Inner>;

pub fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

impl Inner {
    pub fn settings(&self) -> Settings {
        let db = self.db.lock().unwrap();
        db::get_setting(&db, "settings")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn targets(&self, kind: mixlens_core::Kind) -> Targets {
        Targets { kind, target_lufs: self.settings().target_lufs, ..Targets::default() }
    }

    pub fn version(&self, id: i64) -> Result<db::VersionRow, String> {
        let db = self.db.lock().unwrap();
        db::get_version(&db, id).map_err(err)?.ok_or_else(|| format!("version {id} not found"))
    }

    /// Decoded audio for a version, from cache or disk. Fails if the file changed since analysis.
    pub fn audio(&self, id: i64) -> Result<Arc<Audio>, String> {
        if let Some(a) = self.cache.lock().unwrap().get(id) {
            return Ok(a);
        }
        let v = self.version(id)?;
        let a = mixlens_core::decode_file(Path::new(&v.path))
            .map_err(|e| format!("Can't read {} ({e}). Was the file moved or deleted?", v.path))?;
        if a.info.hash != v.report.file.hash {
            return Err(format!(
                "{} has changed on disk since it was analyzed. Add it again as a new version.",
                v.report.file.file_name
            ));
        }
        let a = Arc::new(a);
        self.cache.lock().unwrap().put(id, a.clone());
        Ok(a)
    }
}
