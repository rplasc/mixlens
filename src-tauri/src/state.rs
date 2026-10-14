//! Shared app state and helpers used by both commands and AI tools.

use crate::db;
use crate::settings::Settings;
use mixlens_core::codec::CodecResult;
use mixlens_core::{builtin_platforms, builtin_presets, streaming, Audio, GenrePreset, Kind, MusicalKey, Platform, StreamingReport, Targets};
use rusqlite::Connection;
use std::collections::{HashMap, VecDeque};
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
    /// Latest encode-check results per version (in memory only).
    pub codec_results: Mutex<HashMap<i64, Vec<CodecResult>>>,
}

pub type Shared = Arc<Inner>;

pub fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub fn load_settings(c: &Connection) -> Settings {
    db::get_setting(c, "settings").ok().flatten().map(|s| parse_settings(&s)).unwrap_or_default()
}

/// Settings saved before "main platform" existed keep their behaviour: the default -14 LUFS
/// target becomes Spotify (same target and ceilings), anything else stays a custom target.
pub fn parse_settings(raw: &str) -> Settings {
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(raw) else { return Settings::default() };
    if let Some(o) = v.as_object_mut() {
        if !o.contains_key("main_platform") {
            let t = o.get("target_lufs").and_then(|x| x.as_f64()).unwrap_or(-14.0);
            o.insert("main_platform".into(), (if (t + 14.0).abs() < 1e-6 { "spotify" } else { "custom" }).into());
        }
    }
    serde_json::from_value(v).unwrap_or_default()
}

/// The chosen main platform; None for a custom target.
pub fn main_platform(settings: &Settings) -> Option<Platform> {
    builtin_platforms().into_iter().find(|p| p.id == settings.main_platform)
}

pub fn effective_target(settings: &Settings) -> f64 {
    main_platform(settings).map_or(settings.target_lufs, |p| p.target_lufs)
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

/// Genre and key that apply to a version: a reference carries its own (it's a different
/// song), every other version uses its song's.
pub fn version_context<'a>(project: Option<&'a db::Project>, v: &'a db::VersionRow, kind: Kind) -> (Option<&'a str>, Option<MusicalKey>) {
    if kind == Kind::Reference {
        (v.genre.as_deref(), v.key)
    } else {
        (project.and_then(|p| p.genre.as_deref()), project.and_then(|p| p.key))
    }
}

/// Rules context: the streaming target plus a genre preset id and key.
pub fn build_targets(settings: &Settings, presets: &[GenrePreset], genre: Option<&str>, key: Option<MusicalKey>, kind: Kind) -> Targets {
    let d = Targets::default();
    let platform = main_platform(settings);
    let (target_lufs, ceiling_dbtp, loud_ceiling_dbtp) = match &platform {
        Some(p) => (p.target_lufs, p.ceiling_dbtp, p.loud_ceiling_dbtp.unwrap_or(p.ceiling_dbtp)),
        None => (settings.target_lufs, d.ceiling_dbtp, d.loud_ceiling_dbtp),
    };
    Targets {
        kind,
        target_lufs,
        ceiling_dbtp,
        loud_ceiling_dbtp,
        genre: genre.and_then(|id| presets.iter().find(|g| g.id == id)).cloned(),
        key,
        platform,
    }
}

/// Targets with the song's own context, for a file being added (new references start with
/// the song's genre and key). Uses an already-locked connection.
pub fn targets_in(c: &Connection, project_id: i64, kind: Kind) -> Targets {
    let project = db::get_project(c, project_id).ok().flatten();
    let (genre, key) = (project.as_ref().and_then(|p| p.genre.as_deref()), project.as_ref().and_then(|p| p.key));
    build_targets(&load_settings(c), &load_presets(c), genre, key, kind)
}

/// Targets for an existing version analyzed as `kind`.
pub fn targets_for_row(c: &Connection, v: &db::VersionRow, kind: Kind) -> Targets {
    let project = db::get_project(c, v.project_id).ok().flatten();
    let (genre, key) = version_context(project.as_ref(), v, kind);
    build_targets(&load_settings(c), &load_presets(c), genre, key, kind)
}

impl Inner {
    pub fn new(db: Connection, http: reqwest::Client) -> Self {
        Self { db: Mutex::new(db), cache: Mutex::new(AudioCache::new(2)), http, codec_results: Mutex::new(HashMap::new()) }
    }

    /// Per-platform playback and requirements, using the latest encode check if one ran.
    pub fn streaming(&self, id: i64) -> Result<StreamingReport, String> {
        let v = self.version(id)?;
        let encoded = self.codec_results.lock().unwrap().get(&id).cloned();
        Ok(streaming::assess(&v.report, v.kind, &builtin_platforms(), encoded.as_deref()))
    }

    /// Forget per-version results when versions go away.
    pub fn forget(&self, ids: &[i64]) {
        let mut cache = self.cache.lock().unwrap();
        let mut codec = self.codec_results.lock().unwrap();
        for id in ids {
            cache.remove(*id);
            codec.remove(id);
        }
    }

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
