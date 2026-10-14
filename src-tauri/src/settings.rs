//! App settings. Non-secret settings live in SQLite; API keys live in the OS credential
//! store (Windows Credential Manager) and never reach the web UI.

use serde::{Deserialize, Serialize};

const SERVICE: &str = "MixLens";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Anthropic,
    Openai,
    Ollama,
}

impl Provider {
    pub fn key_name(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic_api_key",
            Provider::Openai => "openai_api_key",
            Provider::Ollama => "ollama",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub provider: Provider,
    pub anthropic_model: String,
    pub openai_model: String,
    pub ollama_model: String,
    pub ollama_url: String,
    pub ffmpeg_path: String,
    /// Platform whose target and ceilings drive the master rules, or "custom" to use `target_lufs`.
    pub main_platform: String,
    /// Loudness target used when `main_platform` is "custom".
    pub target_lufs: f64,
    pub max_tool_rounds: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: Provider::Anthropic,
            anthropic_model: "claude-sonnet-5-5".into(),
            openai_model: String::new(),
            ollama_model: String::new(),
            ollama_url: "http://localhost:11434".into(),
            ffmpeg_path: String::new(),
            main_platform: "spotify".into(),
            target_lufs: -14.0,
            max_tool_rounds: 8,
        }
    }
}

impl Settings {
    pub fn model_for(&self, p: Provider) -> &str {
        match p {
            Provider::Anthropic => &self.anthropic_model,
            Provider::Openai => &self.openai_model,
            Provider::Ollama => &self.ollama_model,
        }
    }
}

/// What the UI sees: settings plus whether keys exist (never the keys themselves).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettingsView {
    #[serde(flatten)]
    pub settings: Settings,
    pub has_anthropic_key: bool,
    pub has_openai_key: bool,
    pub ffmpeg_found: bool,
    pub keyring_ok: bool,
    /// The main platform's target, or the custom one.
    pub effective_target_lufs: f64,
}

fn entry(p: Provider) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, p.key_name()).map_err(|e| e.to_string())
}

pub fn get_key(p: Provider) -> Option<String> {
    entry(p).ok()?.get_password().ok().filter(|k| !k.trim().is_empty())
}

pub fn set_key(p: Provider, key: &str) -> Result<(), String> {
    let e = entry(p)?;
    if key.trim().is_empty() {
        match e.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    } else {
        e.set_password(key.trim()).map_err(|e| e.to_string())
    }
}

pub fn keyring_available() -> bool {
    entry(Provider::Anthropic).is_ok()
}
