// Dependencies needed in Cargo.toml:
// anyhow = "1"
// serde = { version = "1", features = ["derive"] }
// toml = "0.8"

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DuckMode {
    Global,
    Apps,
}

fn default_duck_duration_ms() -> u32 {
    120
}

fn default_restore_duration_ms() -> u32 {
    120
}

fn default_spectral_flatness_threshold() -> f32 {
    0.65
}

fn default_noise_floor_multiplier() -> f32 {
    2.0
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AppConfig {
    pub duck_mode: DuckMode,
    pub duck_ratio: f32,
    pub excluded_apps: Vec<String>,
    pub vad_threshold: f32,
    pub attack_frames: u32,
    pub release_frames: u32,
    #[serde(default = "default_duck_duration_ms")]
    pub duck_duration_ms: u32,
    #[serde(default = "default_restore_duration_ms")]
    pub restore_duration_ms: u32,
    #[serde(default = "default_spectral_flatness_threshold")]
    pub spectral_flatness_threshold: f32,
    #[serde(default = "default_noise_floor_multiplier")]
    pub noise_floor_multiplier: f32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            duck_mode: DuckMode::Global,
            duck_ratio: 0.3,
            excluded_apps: vec![
                "Teams.exe".into(),
                "WeChat.exe".into(),
                "OUTLOOK.EXE".into(),
                "WINWORD.EXE".into(),
            ],
            vad_threshold: 0.5,
            attack_frames: 6,
            release_frames: 30,
            duck_duration_ms: default_duck_duration_ms(),
            restore_duration_ms: default_restore_duration_ms(),
            spectral_flatness_threshold: default_spectral_flatness_threshold(),
            noise_floor_multiplier: default_noise_floor_multiplier(),
        }
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            let content = fs::read_to_string(path)?;
            let config: AppConfig = toml::from_str(&content)?;
            Ok(config)
        } else {
            let config = AppConfig::default();
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let toml_str = toml::to_string_pretty(&config)?;
            fs::write(path, toml_str)?;
            Ok(config)
        }
    }

    pub fn config_dir() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn config_file_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp_path = path.with_extension("toml.tmp");
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&tmp_path, &content)?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }
}

/// Validate process name: only allow alphanumeric, underscore, dot, hyphen
pub fn validate_process_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-')
}
