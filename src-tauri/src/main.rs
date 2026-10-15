// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai;
mod commands;
mod db;
mod settings;
mod state;
#[cfg(test)]
mod tests;

use state::Inner;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = db::open(&dir.join("mixlens.db"))?;
            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(180))
                .user_agent(concat!("MixLens/", env!("CARGO_PKG_VERSION")))
                .build()?;
            app.manage(Arc::new(Inner::new(conn, http)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::create_project,
            commands::rename_project,
            commands::delete_project,
            commands::list_genre_presets,
            commands::update_project_context,
            commands::list_versions,
            commands::add_version,
            commands::update_version,
            commands::update_version_context,
            commands::delete_version,
            commands::get_report,
            commands::compare_versions,
            commands::compare_reference,
            commands::section_analysis,
            commands::codec_check,
            commands::list_platforms,
            commands::streaming_report,
            commands::export_report,
            commands::get_settings,
            commands::save_settings,
            commands::set_api_key,
            commands::list_models,
            commands::get_chat,
            commands::clear_chat,
            commands::ai_chat,
        ])
        .run(tauri::generate_context!())
        .expect("error while running MixLens");
}
