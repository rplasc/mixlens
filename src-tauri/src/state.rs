//! Shared app state and helpers used by both commands and AI tools.

use crate::db;
use crate::settings::Settings;
use mixlens_core::{builtin_presets, Audio, GenrePreset, Kind, Targets};
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

pub fn load_settings(c: &Connection) -> Settings {
    db::get_setting(c, "settings").ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Built-in genre presets, with user presets (settings key `genre_presets`) layered on top:
/// a user preset with a built-in's id replaces it.
pub fn load_presets(c: &Connection) -> Vec<GenrePreset> {
    let user: Vec<GenrePreset> =
        db::get_setting(c, "genre_presets").ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let mut out = builtin_presets();
    for mut p in user {
        p.builtin = false;
        match out.iter_mut().find(|b| b.id == p.id) {
            Some(slot) => *slot = p,
            None => out.push(p),
        }
    }
    out
}

/// Rules context for a version: the streaming target plus its song's genre and key.
pub fn build_targets(settings: &Settings, presets: &[GenrePreset], project: Option<&db::Project>, kind: Kind) -> Targets {
    Targets {
        kind,
        target_lufs: settings.target_lufs,
        genre: project.and_then(|p| p.genre.as_deref()).and_then(|id| presets.iter().find(|g| g.id == id)).cloned(),
        key: project.and_then(|p| p.key),
        ..Targets::default()
    }
}

/// Targets for a version, using an already-locked connection.
pub fn targets_in(c: &Connection, project_id: i64, kind: Kind) -> Targets {
    let project = db::get_project(c, project_id).ok().flatten();
    build_targets(&load_settings(c), &load_presets(c), project.as_ref(), kind)
}

impl Inner {
    pub fn settings(&self) -> Settings {
        load_settings(&self.db.lock().unwrap())
    }

    pub fn targets_for(&self, project_id: i64, kind: Kind) -> Targets {
        targets_in(&self.db.lock().unwrap(), project_id, kind)
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
