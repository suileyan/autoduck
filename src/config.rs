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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AppConfig {
    pub duck_mode: DuckMode,
    pub duck_ratio: f32,
    pub excluded_apps: Vec<String>,
    pub vad_threshold: f32,
    pub attack_frames: u32,
    pub release_frames: u32,
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
            attack_frames: 4,
            release_frames: 30,
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
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
        PathBuf::from(appdata).join("AutoDuck")
    }

    pub fn config_file_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }
}
