//! Tauri commands exposed to the Angular UI.

use crate::ai;
use crate::db::{self, Message, Project, VersionSummary};
use crate::settings::{self, Provider, Settings, SettingsView};
use crate::state::{self, err, Shared};
use mixlens_core::{codec, compare, reference_compare, refresh_findings, tools, GenrePreset, Kind, MusicalKey};
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
    db::delete_project(&state.db.lock().unwrap(), id).map_err(err)
}

/// Re-run the (cheap) rules for every version of a project with its current context.
fn refresh_project(c: &rusqlite::Connection, project: &Project, settings: &Settings, presets: &[GenrePreset]) -> R<()> {
    for mut v in db::list_versions(c, project.id).map_err(err)? {
        refresh_findings(&mut v.report, &state::build_targets(settings, presets, Some(project), v.kind));
        db::update_version(c, v.id, &v.label, v.kind, &v.report).map_err(err)?;
    }
    Ok(())
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
        let genre = genre.filter(|g| !g.is_empty());
        if let Some(g) = &genre {
            if !presets.iter().any(|p| &p.id == g) {
                return Err(format!("unknown genre preset '{g}'"));
            }
        }
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
        .map(|v| db::summarize(v.id, v.project_id, v.label, v.path, v.kind, v.created_at, &v.report))
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
            db::insert_version(&c, project_id, &label, &path, kind, &report).map_err(err)?
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

#[tauri::command]
pub fn update_version(state: State<'_, Shared>, id: i64, label: String, kind: Kind) -> R<VersionSummary> {
    let mut v = state.version(id)?;
    refresh_findings(&mut v.report, &state.targets_for(v.project_id, kind));
    let c = state.db.lock().unwrap();
    db::update_version(&c, id, label.trim(), kind, &v.report).map_err(err)?;
    Ok(db::summarize(id, v.project_id, label.trim().to_string(), v.path, kind, v.created_at, &v.report))
}

#[tauri::command]
pub fn delete_version(state: State<'_, Shared>, id: i64) -> R<()> {
    state.cache.lock().unwrap().remove(id);
    db::delete_version(&state.db.lock().unwrap(), id).map_err(err)
}

#[tauri::command]
pub fn get_report(state: State<'_, Shared>, id: i64) -> R<Value> {
    let v = state.version(id)?;
    let mut out = serde_json::to_value(&v.report).map_err(err)?;
    if let Some(o) = out.as_object_mut() {
        o.remove("kblocks");
    }
    Ok(json!({"id": v.id, "label": v.label, "kind": v.kind, "path": v.path, "report": out}))
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

// ---------- settings ----------

fn settings_view(s: &Shared) -> SettingsView {
    let st = s.settings();
    let ff = codec::find_ffmpeg(Some(st.ffmpeg_path.as_str()).filter(|p| !p.is_empty())).is_some();
    SettingsView {
        settings: st,
        has_anthropic_key: settings::get_key(Provider::Anthropic).is_some(),
        has_openai_key: settings::get_key(Provider::Openai).is_some(),
        ffmpeg_found: ff,
        keyring_ok: settings::keyring_available(),
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
        if (old.target_lufs - settings.target_lufs).abs() > 1e-9 {
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
