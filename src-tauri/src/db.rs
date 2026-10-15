//! SQLite persistence: projects, versions (with cached reports), chat history, settings.

use mixlens_core::{Kind, MusicalKey, Report, Severity};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS projects (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
         );
         CREATE TABLE IF NOT EXISTS versions (
            id INTEGER PRIMARY KEY,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            label TEXT NOT NULL,
            path TEXT NOT NULL,
            hash TEXT NOT NULL,
            kind TEXT NOT NULL,
            report_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
         );
         CREATE INDEX IF NOT EXISTS idx_versions_project ON versions(project_id);
         CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
         );
         CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
         );",
    )?;
    migrate(&conn)?;
    Ok(conn)
}

/// Schema upgrades, tracked with `PRAGMA user_version`.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        // Song context: genre preset id and key (JSON of `MusicalKey`).
        conn.execute_batch(
            "BEGIN;
             ALTER TABLE projects ADD COLUMN genre TEXT;
             ALTER TABLE projects ADD COLUMN song_key TEXT;
             PRAGMA user_version = 1;
             COMMIT;",
        )?;
    }
    if version < 2 {
        // Reference tracks carry their own genre and key, since they can differ from the song's.
        conn.execute_batch(
            "BEGIN;
             ALTER TABLE versions ADD COLUMN genre TEXT;
             ALTER TABLE versions ADD COLUMN song_key TEXT;
             PRAGMA user_version = 2;
             COMMIT;",
        )?;
    }
    if version < 3 {
        // The user's own notes on a version.
        conn.execute_batch(
            "BEGIN;
             ALTER TABLE versions ADD COLUMN notes TEXT NOT NULL DEFAULT '';
             PRAGMA user_version = 3;
             COMMIT;",
        )?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub version_count: i64,
    pub created_at: String,
    /// Genre preset id.
    pub genre: Option<String>,
    pub key: Option<MusicalKey>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VersionSummary {
    pub id: i64,
    pub project_id: i64,
    pub label: String,
    pub file_name: String,
    pub path: String,
    pub kind: Kind,
    pub created_at: String,
    /// Own genre preset id and key; only used for references (other versions use the song's).
    pub genre: Option<String>,
    pub key: Option<MusicalKey>,
    /// The user's own notes; never exported.
    pub notes: String,
    pub integrated_lufs: f64,
    pub true_peak_dbtp: f64,
    pub lra_lu: f64,
    pub plr_db: f64,
    pub correlation: f64,
    pub duration_s: f64,
    pub sample_rate: u32,
    pub sample_format: String,
    pub worst: Severity,
    pub errors: usize,
    pub warnings: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

pub fn kind_str(k: Kind) -> &'static str {
    match k {
        Kind::Mix => "mix",
        Kind::Master => "master",
        Kind::Reference => "reference",
    }
}

pub fn parse_kind(s: &str) -> Kind {
    match s {
        "master" => Kind::Master,
        "reference" => Kind::Reference,
        _ => Kind::Mix,
    }
}

pub fn summarize(v: &VersionRow) -> VersionSummary {
    let r = &v.report;
    let errors = r.findings.iter().filter(|f| f.severity == Severity::Error).count();
    let warnings = r.findings.iter().filter(|f| f.severity == Severity::Warn).count();
    VersionSummary {
        id: v.id,
        project_id: v.project_id,
        label: v.label.clone(),
        file_name: r.file.file_name.clone(),
        path: v.path.clone(),
        kind: v.kind,
        created_at: v.created_at.clone(),
        genre: v.genre.clone(),
        key: v.key,
        notes: v.notes.clone(),
        integrated_lufs: r.loudness.integrated_lufs,
        true_peak_dbtp: r.loudness.true_peak_dbtp,
        lra_lu: r.loudness.lra_lu,
        plr_db: r.loudness.plr_db,
        correlation: r.stereo.correlation,
        duration_s: r.file.duration_s,
        sample_rate: r.file.sample_rate,
        sample_format: r.file.sample_format.clone(),
        worst: r.findings.iter().map(|f| f.severity).max().unwrap_or(Severity::Ok),
        errors,
        warnings,
    }
}

const PROJECT_COLS: &str = "p.id, p.name, p.created_at, (SELECT COUNT(*) FROM versions v WHERE v.project_id = p.id), p.genre, p.song_key";

fn parse_key(json: Option<String>) -> Option<MusicalKey> {
    json.and_then(|k| serde_json::from_str(&k).ok())
}

fn key_json(key: Option<&MusicalKey>) -> Option<String> {
    key.map(|k| serde_json::to_string(k).expect("key serializes"))
}

fn row_to_project(r: &rusqlite::Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: r.get(0)?,
        name: r.get(1)?,
        created_at: r.get(2)?,
        version_count: r.get(3)?,
        genre: r.get(4)?,
        key: parse_key(r.get(5)?),
    })
}

pub fn list_projects(c: &Connection) -> rusqlite::Result<Vec<Project>> {
    let mut st = c.prepare(&format!("SELECT {PROJECT_COLS} FROM projects p ORDER BY p.id DESC"))?;
    let rows = st.query_map([], row_to_project)?;
    rows.collect()
}

pub fn get_project(c: &Connection, id: i64) -> rusqlite::Result<Option<Project>> {
    c.query_row(&format!("SELECT {PROJECT_COLS} FROM projects p WHERE p.id = ?1"), params![id], row_to_project).optional()
}

pub fn create_project(c: &Connection, name: &str) -> rusqlite::Result<Project> {
    c.execute("INSERT INTO projects (name) VALUES (?1)", params![name])?;
    Ok(get_project(c, c.last_insert_rowid())?.expect("just inserted"))
}

pub fn set_project_context(c: &Connection, id: i64, genre: Option<&str>, key: Option<&MusicalKey>) -> rusqlite::Result<()> {
    c.execute("UPDATE projects SET genre = ?1, song_key = ?2 WHERE id = ?3", params![genre, key_json(key), id]).map(|_| ())
}

pub fn rename_project(c: &Connection, id: i64, name: &str) -> rusqlite::Result<()> {
    c.execute("UPDATE projects SET name = ?1 WHERE id = ?2", params![name, id]).map(|_| ())
}

pub fn delete_project(c: &Connection, id: i64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM projects WHERE id = ?1", params![id]).map(|_| ())
}

pub fn insert_version(c: &Connection, project_id: i64, label: &str, path: &str, kind: Kind, r: &Report) -> rusqlite::Result<VersionSummary> {
    let json = serde_json::to_string(r).expect("report serializes");
    c.execute(
        "INSERT INTO versions (project_id, label, path, hash, kind, report_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![project_id, label, path, r.file.hash, kind_str(kind), json],
    )?;
    Ok(summarize(&get_version(c, c.last_insert_rowid())?.expect("just inserted")))
}

pub fn set_version_context(c: &Connection, id: i64, genre: Option<&str>, key: Option<&MusicalKey>) -> rusqlite::Result<()> {
    c.execute("UPDATE versions SET genre = ?1, song_key = ?2 WHERE id = ?3", params![genre, key_json(key), id]).map(|_| ())
}

pub struct VersionRow {
    pub id: i64,
    pub project_id: i64,
    pub label: String,
    pub path: String,
    pub kind: Kind,
    pub created_at: String,
    pub genre: Option<String>,
    pub key: Option<MusicalKey>,
    pub notes: String,
    pub report: Report,
}

fn row_to_version(r: &rusqlite::Row) -> rusqlite::Result<VersionRow> {
    let json: String = r.get(5)?;
    let report: Report = serde_json::from_str(&json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e)))?;
    Ok(VersionRow {
        id: r.get(0)?,
        project_id: r.get(1)?,
        label: r.get(2)?,
        path: r.get(3)?,
        kind: parse_kind(&r.get::<_, String>(4)?),
        report,
        created_at: r.get(6)?,
        genre: r.get(7)?,
        key: parse_key(r.get(8)?),
        notes: r.get(9)?,
    })
}

const VERSION_COLS: &str = "id, project_id, label, path, kind, report_json, created_at, genre, song_key, notes";

pub fn get_version(c: &Connection, id: i64) -> rusqlite::Result<Option<VersionRow>> {
    c.query_row(&format!("SELECT {VERSION_COLS} FROM versions WHERE id = ?1"), params![id], row_to_version).optional()
}

pub fn list_versions(c: &Connection, project_id: i64) -> rusqlite::Result<Vec<VersionRow>> {
    let mut st = c.prepare(&format!("SELECT {VERSION_COLS} FROM versions WHERE project_id = ?1 ORDER BY id ASC"))?;
    let rows = st.query_map(params![project_id], row_to_version)?;
    rows.collect()
}


pub fn update_version(c: &Connection, id: i64, label: &str, kind: Kind, r: &Report) -> rusqlite::Result<()> {
    let json = serde_json::to_string(r).expect("report serializes");
    c.execute(
        "UPDATE versions SET label = ?1, kind = ?2, report_json = ?3 WHERE id = ?4",
        params![label, kind_str(kind), json, id],
    )
    .map(|_| ())
}

pub fn set_version_notes(c: &Connection, id: i64, notes: &str) -> rusqlite::Result<()> {
    c.execute("UPDATE versions SET notes = ?1 WHERE id = ?2", params![notes, id]).map(|_| ())
}

pub fn delete_version(c: &Connection, id: i64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM versions WHERE id = ?1", params![id]).map(|_| ())
}

pub fn messages(c: &Connection, project_id: i64) -> rusqlite::Result<Vec<Message>> {
    let mut st = c.prepare("SELECT id, role, content, created_at FROM messages WHERE project_id = ?1 ORDER BY id ASC")?;
    let rows = st.query_map(params![project_id], |r| Ok(Message { id: r.get(0)?, role: r.get(1)?, content: r.get(2)?, created_at: r.get(3)? }))?;
    rows.collect()
}

pub fn add_message(c: &Connection, project_id: i64, role: &str, content: &str) -> rusqlite::Result<Message> {
    c.execute("INSERT INTO messages (project_id, role, content) VALUES (?1, ?2, ?3)", params![project_id, role, content])?;
    let id = c.last_insert_rowid();
    c.query_row("SELECT id, role, content, created_at FROM messages WHERE id = ?1", params![id], |r| {
        Ok(Message { id: r.get(0)?, role: r.get(1)?, content: r.get(2)?, created_at: r.get(3)? })
    })
}

pub fn clear_messages(c: &Connection, project_id: i64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM messages WHERE project_id = ?1", params![project_id]).map(|_| ())
}

pub fn get_setting(c: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    c.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0)).optional()
}

pub fn set_setting(c: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map(|_| ())
}
