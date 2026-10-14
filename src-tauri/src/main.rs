// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai;
mod commands;
mod db;
mod settings;
mod state;
#[cfg(test)]
mod tests;

use state::{AudioCache, Inner};
use std::sync::{Arc, Mutex};
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
            app.manage(Arc::new(Inner { db: Mutex::new(conn), cache: Mutex::new(AudioCache::new(2)), http }));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::create_project,
            commands::rename_project,
            commands::delete_project,
            commands::list_versions,
            commands::add_version,
            commands::update_version,
            commands::delete_version,
            commands::get_report,
            commands::compare_versions,
            commands::section_analysis,
            commands::codec_check,
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
