//! Tauri commands exposed to the Angular UI.

use crate::ai;
use crate::db::{self, Message, Project, VersionSummary};
use crate::settings::{self, Provider, Settings, SettingsView};
use crate::state::{self, err, Shared};
use mixlens_core::{codec, compare, export, reference_compare, refresh_findings, tools, Audio, ExportContext, GenrePreset, Kind, MusicalKey};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, State};

type R<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> R<T> + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(err)?
}

// ---------- projects ----------

#[tauri::command]
pub fn list_projects(state: State<'_, Shared>) -> R<Vec<Project>> {
    db::list_projects(&state.db.lock().unwrap()).map_err(err)
}

#[tauri::command]
pub fn create_project(state: State<'_, Shared>, name: String) -> R<Project> {
    let name = if name.trim().is_empty() { "Untitled project".to_string() } else { name.trim().to_string() };
    db::create_project(&state.db.lock().unwrap(), &name).map_err(err)
}

#[tauri::command]
pub fn rename_project(state: State<'_, Shared>, id: i64, name: String) -> R<()> {
    db::rename_project(&state.db.lock().unwrap(), id, name.trim()).map_err(err)
}

#[tauri::command]
pub fn delete_project(state: State<'_, Shared>, id: i64) -> R<()> {
    let c = state.db.lock().unwrap();
    // Versions and chat go with the project (ON DELETE CASCADE); drop their decoded audio too.
    let ids: Vec<i64> = db::list_versions(&c, id).map_err(err)?.into_iter().map(|v| v.id).collect();
    db::delete_project(&c, id).map_err(err)?;
    state.forget(&ids);
    Ok(())
}

/// Re-run the (cheap) rules for every version of a project with its current context.
pub fn refresh_project(c: &rusqlite::Connection, project: &Project, settings: &Settings, presets: &[GenrePreset]) -> R<()> {
    for mut v in db::list_versions(c, project.id).map_err(err)? {
        let (genre, key) = state::version_context(Some(project), &v, v.kind);
        let targets = state::build_targets(settings, presets, genre, key, v.kind);
        refresh_findings(&mut v.report, &targets);
        db::update_version(c, v.id, &v.label, v.kind, &v.report).map_err(err)?;
    }
    Ok(())
}

/// Empty means "not set"; anything else must name a known preset.
fn check_genre(presets: &[GenrePreset], genre: Option<String>) -> R<Option<String>> {
    match genre.filter(|g| !g.is_empty()) {
        Some(g) if !presets.iter().any(|p| p.id == g) => Err(format!("unknown genre preset '{g}'")),
        g => Ok(g),
    }
}

#[tauri::command]
pub fn list_genre_presets(state: State<'_, Shared>) -> R<Vec<GenrePreset>> {
    Ok(state::load_presets(&state.db.lock().unwrap()))
}

/// Set a song's genre and key, then refresh every version's findings.
#[tauri::command]
pub async fn update_project_context(state: State<'_, Shared>, id: i64, genre: Option<String>, key: Option<MusicalKey>) -> R<Project> {
    let s: Shared = state.inner().clone();
    blocking(move || {
        let c = s.db.lock().unwrap();
        let presets = state::load_presets(&c);
        let genre = check_genre(&presets, genre)?;
        db::set_project_context(&c, id, genre.as_deref(), key.as_ref()).map_err(err)?;
        let project = db::get_project(&c, id).map_err(err)?.ok_or_else(|| format!("project {id} not found"))?;
        refresh_project(&c, &project, &state::load_settings(&c), &presets)?;
        Ok(project)
    })
    .await
}

// ---------- versions ----------

#[tauri::command]
pub fn list_versions(state: State<'_, Shared>, project_id: i64) -> R<Vec<VersionSummary>> {
    let c = state.db.lock().unwrap();
    Ok(db::list_versions(&c, project_id)
        .map_err(err)?
        .into_iter()
        .map(|v| db::summarize(&v))
        .collect())
}

#[tauri::command]
pub async fn add_version(state: State<'_, Shared>, project_id: i64, path: String, kind: Option<Kind>, label: Option<String>) -> R<VersionSummary> {
    let s: Shared = state.inner().clone();
    blocking(move || {
        let p = Path::new(&path);
        let name = p.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "version".into());
        let kind = kind.unwrap_or_else(|| guess_kind(&name));
        let label = label.filter(|l| !l.trim().is_empty()).unwrap_or(name);
        let targets = s.targets_for(project_id, kind);
        let (audio, report) = mixlens_core::analyze_file(p, &targets).map_err(err)?;
        let summary = {
            let c = s.db.lock().unwrap();
            let mut summary = db::insert_version(&c, project_id, &label, &path, kind, &report).map_err(err)?;
            if kind == Kind::Reference {
                // A new reference starts with the song's genre and key; it can be changed on its own.
                if let Some(p) = db::get_project(&c, project_id).map_err(err)? {
                    db::set_version_context(&c, summary.id, p.genre.as_deref(), p.key.as_ref()).map_err(err)?;
                    (summary.genre, summary.key) = (p.genre, p.key);
                }
            }
            summary
        };
        s.cache.lock().unwrap().put(summary.id, Arc::new(audio));
        Ok(summary)
    })
    .await
}

/// Kind from a file name: "ref"/"reference" as a separate word, then "master", else a mix.
pub fn guess_kind(stem: &str) -> Kind {
    let lower = stem.to_lowercase();
    if lower.split(|c: char| !c.is_alphanumeric()).any(|w| w == "ref" || w == "reference") {
        Kind::Reference
    } else if lower.contains("master") {
        Kind::Master
    } else {
        Kind::Mix
    }
}

/// Relabel a version or change its kind, and re-run its rules. A version that becomes a
/// reference without a genre or key of its own starts with the song's.
pub fn set_version_kind(c: &rusqlite::Connection, id: i64, label: &str, kind: Kind) -> R<VersionSummary> {
    let mut v = db::get_version(c, id).map_err(err)?.ok_or_else(|| format!("version {id} not found"))?;
    if kind == Kind::Reference && v.kind != Kind::Reference && v.genre.is_none() && v.key.is_none() {
        if let Some(p) = db::get_project(c, v.project_id).map_err(err)? {
            db::set_version_context(c, id, p.genre.as_deref(), p.key.as_ref()).map_err(err)?;
            (v.genre, v.key) = (p.genre, p.key);
        }
    }
    let targets = state::targets_for_row(c, &v, kind);
    refresh_findings(&mut v.report, &targets);
    (v.label, v.kind) = (label.to_string(), kind);
    db::update_version(c, id, label, kind, &v.report).map_err(err)?;
    Ok(db::summarize(&v))
}

#[tauri::command]
pub fn update_version(state: State<'_, Shared>, id: i64, label: String, kind: Kind) -> R<VersionSummary> {
    set_version_kind(&state.db.lock().unwrap(), id, label.trim(), kind)
}

/// Set a reference track's own genre and key, then refresh its findings.
pub fn set_version_context(c: &rusqlite::Connection, id: i64, genre: Option<String>, key: Option<MusicalKey>) -> R<VersionSummary> {
    let genre = check_genre(&state::load_presets(c), genre)?;
    let mut v = db::get_version(c, id).map_err(err)?.ok_or_else(|| format!("version {id} not found"))?;
    if v.kind != Kind::Reference {
        return Err("Only reference tracks have their own genre and key; this version uses the song's.".into());
    }
    db::set_version_context(c, id, genre.as_deref(), key.as_ref()).map_err(err)?;
    (v.genre, v.key) = (genre, key);
    let targets = state::targets_for_row(c, &v, v.kind);
    refresh_findings(&mut v.report, &targets);
    db::update_version(c, id, &v.label, v.kind, &v.report).map_err(err)?;
    Ok(db::summarize(&v))
}

#[tauri::command]
pub fn update_version_context(state: State<'_, Shared>, id: i64, genre: Option<String>, key: Option<MusicalKey>) -> R<VersionSummary> {
    set_version_context(&state.db.lock().unwrap(), id, genre, key)
}

/// Longest note kept, in characters.
const MAX_NOTES: usize = 4000;

/// Save the user's notes on a version.
pub fn set_version_notes(c: &rusqlite::Connection, id: i64, notes: &str) -> R<VersionSummary> {
    let notes: String = notes.trim().chars().take(MAX_NOTES).collect();
    db::set_version_notes(c, id, &notes).map_err(err)?;
    let v = db::get_version(c, id).map_err(err)?.ok_or_else(|| format!("version {id} not found"))?;
    Ok(db::summarize(&v))
}

#[tauri::command]
pub fn update_version_notes(state: State<'_, Shared>, id: i64, notes: String) -> R<VersionSummary> {
    set_version_notes(&state.db.lock().unwrap(), id, &notes)
}

#[tauri::command]
pub fn delete_version(state: State<'_, Shared>, id: i64) -> R<()> {
    state.forget(&[id]);
    db::delete_version(&state.db.lock().unwrap(), id).map_err(err)
}

#[tauri::command]
pub fn get_report(state: State<'_, Shared>, id: i64) -> R<Value> {
    let v = state.version(id)?;
    let mut out = serde_json::to_value(&v.report).map_err(err)?;
    if let Some(o) = out.as_object_mut() {
        o.remove("kblocks");
        // Per-block peaks are only for deriving `peak_marks`.
        if let Some(l) = o.get_mut("loudness").and_then(|l| l.as_object_mut()) {
            l.remove("peak_map");
        }
    }
    let peaks_located = v.report.loudness.peak_map.is_some();
    Ok(json!({"id": v.id, "label": v.label, "kind": v.kind, "path": v.path, "peaks_located": peaks_located, "report": out}))
}

/// Record peak positions for a version analyzed before they were kept, by re-analyzing the file.
/// Fails if the file has moved or changed since it was added.
pub fn locate_peaks_in(s: &Shared, id: i64) -> R<VersionSummary> {
    let v = s.version(id)?;
    if v.report.loudness.peak_map.is_some() {
        return Ok(db::summarize(&v));
    }
    let audio = s.audio(id)?;
    let targets = state::targets_for_row(&s.db.lock().unwrap(), &v, v.kind);
    let fresh = mixlens_core::analyze::analyze(&audio, &targets).map_err(err)?;
    // Re-read: the version may have been renamed or retyped while the file was analyzed.
    let c = s.db.lock().unwrap();
    let mut v = db::get_version(&c, id).map_err(err)?.ok_or_else(|| format!("version {id} not found"))?;
    v.report.loudness.peak_map = fresh.loudness.peak_map;
    let targets = state::targets_for_row(&c, &v, v.kind);
    refresh_findings(&mut v.report, &targets);
    db::update_version(&c, id, &v.label, v.kind, &v.report).map_err(err)?;
    Ok(db::summarize(&v))
}

#[tauri::command]
pub async fn locate_peaks(state: State<'_, Shared>, id: i64) -> R<VersionSummary> {
    let s: Shared = state.inner().clone();
    blocking(move || locate_peaks_in(&s, id)).await
}

#[tauri::command]
pub async fn compare_versions(state: State<'_, Shared>, a: i64, b: i64) -> R<Value> {
    let s: Shared = state.inner().clone();
    blocking(move || {
        let (va, vb) = (s.version(a)?, s.version(b)?);
        let (aa, ab) = (s.audio(a)?, s.audio(b)?);
        let c = compare(&aa, &va.report, &ab, &vb.report).map_err(err)?;
        let mut v = serde_json::to_value(&c).map_err(err)?;
        v["octave_bands"] = json!(mixlens_core::compare::octave_band_labels());
        Ok(v)
    })
    .await
}

/// A version against a reference track. Either argument may be the reference.
#[tauri::command]
pub async fn compare_reference(state: State<'_, Shared>, a: i64, b: i64) -> R<Value> {
    let s: Shared = state.inner().clone();
    blocking(move || {
        let (va, vb) = (s.version(a)?, s.version(b)?);
        let (version, reference) = if va.kind == Kind::Reference && vb.kind != Kind::Reference { (vb, va) } else { (va, vb) };
        let c = reference_compare(&version.report, &reference.report);
        let mut v = serde_json::to_value(&c).map_err(err)?;
        v["version_id"] = json!(version.id);
        v["reference_id"] = json!(reference.id);
        Ok(v)
    })
    .await
}

/// Decoded audio for the A/B player: sample rate and frame count (u32 LE), then the left
/// channel and the right channel as f32 LE.
pub fn pcm_bytes(a: &Audio) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + a.len() * 8);
    out.extend_from_slice(&a.sample_rate.to_le_bytes());
    out.extend_from_slice(&(a.len() as u32).to_le_bytes());
    for x in a.left.iter().chain(&a.right) {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

#[tauri::command]
pub async fn playback_audio(state: State<'_, Shared>, id: i64) -> R<tauri::ipc::Response> {
    let s: Shared = state.inner().clone();
    blocking(move || {
        let audio = s.audio(id)?;
        Ok(tauri::ipc::Response::new(pcm_bytes(&audio)))
    })
    .await
}

#[tauri::command]
pub async fn section_analysis(state: State<'_, Shared>, id: i64, start_s: f64, end_s: f64) -> R<Value> {
    let s: Shared = state.inner().clone();
    blocking(move || {
        let v = s.version(id)?;
        let a = s.audio(id)?;
        Ok(json!({
            "loudness": tools::section_loudness(&v.report, start_s, end_s),
            "bands": tools::band_energy(&a, start_s, end_s),
            "octave_bands": mixlens_core::compare::octave_band_labels(),
        }))
    })
    .await
}

#[tauri::command]
pub async fn codec_check(state: State<'_, Shared>, id: i64) -> R<Value> {
    let s: Shared = state.inner().clone();
    blocking(move || ai::tools::execute(&s, "codec_check", &json!({"version_id": id}))).await
}

#[tauri::command]
pub fn list_platforms() -> Vec<mixlens_core::Platform> {
    mixlens_core::builtin_platforms()
}

/// How each platform will play this version, using the latest encode check if one ran.
#[tauri::command]
pub fn streaming_report(state: State<'_, Shared>, id: i64) -> R<mixlens_core::StreamingReport> {
    state.streaming(id)
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Pdf,
    Markdown,
}

impl ExportFormat {
    /// Accepted extensions; the first is added when a path has none of them.
    fn extensions(self) -> &'static [&'static str] {
        match self {
            ExportFormat::Pdf => &["pdf"],
            ExportFormat::Markdown => &["md", "markdown"],
        }
    }
}

/// Write the shareable report for a version as `format`, adding the format's extension if the
/// path doesn't end in one. Returns the path written.
pub fn export_report_to(s: &Shared, id: i64, path: &str, format: ExportFormat) -> R<String> {
    let v = s.version(id)?;
    let streaming = s.streaming(id)?;
    let (song, targets) = {
        let c = s.db.lock().unwrap();
        let song = db::get_project(&c, v.project_id).map_err(err)?.map_or_else(|| "Untitled song".to_string(), |p| p.name);
        (song, state::targets_for_row(&c, &v, v.kind))
    };
    let ctx = ExportContext {
        song,
        version: v.label.clone(),
        // SQLite datetime: "2026-10-15 09:30:00".
        analyzed_on: v.created_at.split(' ').next().unwrap_or_default().to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        targets,
    };
    let doc = export::build(&ctx, &v.report, &streaming);
    let bytes = match format {
        ExportFormat::Pdf => doc.to_pdf().map_err(err)?,
        ExportFormat::Markdown => doc.to_markdown().into_bytes(),
    };
    let exts = format.extensions();
    let has_ext = Path::new(path).extension().is_some_and(|e| exts.iter().any(|x| e.eq_ignore_ascii_case(x)));
    let path = if has_ext { path.to_string() } else { format!("{path}.{}", exts[0]) };
    std::fs::write(&path, bytes).map_err(|e| format!("Couldn't save the report to {path} ({e})."))?;
    Ok(path)
}

#[tauri::command]
pub async fn export_report(state: State<'_, Shared>, id: i64, path: String, format: ExportFormat) -> R<String> {
    let s: Shared = state.inner().clone();
    blocking(move || export_report_to(&s, id, &path, format)).await
}

// ---------- settings ----------

fn settings_view(s: &Shared) -> SettingsView {
    let st = s.settings();
    let ff = codec::find_ffmpeg(Some(st.ffmpeg_path.as_str()).filter(|p| !p.is_empty())).is_some();
    let effective_target_lufs = state::effective_target(&st);
    SettingsView {
        settings: st,
        has_anthropic_key: settings::get_key(Provider::Anthropic).is_some(),
        has_openai_key: settings::get_key(Provider::Openai).is_some(),
        ffmpeg_found: ff,
        keyring_ok: settings::keyring_available(),
        effective_target_lufs,
    }
}

#[tauri::command]
pub async fn get_settings(state: State<'_, Shared>) -> R<SettingsView> {
    let s: Shared = state.inner().clone();
    blocking(move || Ok(settings_view(&s))).await
}

#[tauri::command]
pub async fn save_settings(state: State<'_, Shared>, settings: Settings) -> R<SettingsView> {
    let s: Shared = state.inner().clone();
    blocking(move || {
        let old = s.settings();
        {
            let c = s.db.lock().unwrap();
            db::set_setting(&c, "settings", &serde_json::to_string(&settings).map_err(err)?).map_err(err)?;
        }
        // Target loudness affects findings; re-run the (cheap) rules for every version.
        if (old.target_lufs - settings.target_lufs).abs() > 1e-9 || old.main_platform != settings.main_platform {
            let c = s.db.lock().unwrap();
            let presets = state::load_presets(&c);
            for p in db::list_projects(&c).map_err(err)? {
                refresh_project(&c, &p, &settings, &presets)?;
            }
        }
        Ok(settings_view(&s))
    })
    .await
}

#[tauri::command]
pub async fn set_api_key(state: State<'_, Shared>, provider: Provider, key: String) -> R<SettingsView> {
    let s: Shared = state.inner().clone();
    blocking(move || {
        settings::set_key(provider, &key)?;
        Ok(settings_view(&s))
    })
    .await
}

#[tauri::command]
pub async fn list_models(state: State<'_, Shared>, provider: Provider) -> R<Vec<String>> {
    let st = state.settings();
    let key = match provider {
        Provider::Ollama => None,
        p => settings::get_key(p),
    };
    ai::providers::list_models(&state.http, provider, key.as_deref(), &st.ollama_url).await
}

// ---------- AI chat ----------

#[tauri::command]
pub fn get_chat(state: State<'_, Shared>, project_id: i64) -> R<Vec<Message>> {
    db::messages(&state.db.lock().unwrap(), project_id).map_err(err)
}

#[tauri::command]
pub fn clear_chat(state: State<'_, Shared>, project_id: i64) -> R<()> {
    db::clear_messages(&state.db.lock().unwrap(), project_id).map_err(err)
}

#[tauri::command]
pub async fn ai_chat(state: State<'_, Shared>, app: AppHandle, project_id: i64, message: String, focus: Vec<i64>) -> R<Message> {
    ai::run(state.inner().clone(), app, project_id, message, focus).await
}
