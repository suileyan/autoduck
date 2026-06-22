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

fn default_enabled() -> bool {
    true
}

fn default_hotkey() -> String {
    "Ctrl+Shift+D".to_string()
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
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
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
            enabled: default_enabled(),
            hotkey: default_hotkey(),
        }
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            let content = fs::read_to_string(path)?;
            let mut config: AppConfig = toml::from_str(&content)?;
            config.validate();
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

    /// 验证并修正配置字段值，非法值使用默认值替代
    fn validate(&mut self) {
        let defaults = AppConfig::default();
        if self.duck_ratio <= 0.0 || self.duck_ratio > 1.0 {
            self.duck_ratio = defaults.duck_ratio;
        }
        if self.vad_threshold < 0.0 || self.vad_threshold > 1.0 {
            self.vad_threshold = defaults.vad_threshold;
        }
        if self.attack_frames == 0 {
            self.attack_frames = defaults.attack_frames;
        }
        if self.release_frames == 0 {
            self.release_frames = defaults.release_frames;
        }
        if self.spectral_flatness_threshold < 0.0 || self.spectral_flatness_threshold > 1.0 {
            self.spectral_flatness_threshold = defaults.spectral_flatness_threshold;
        }
        if self.noise_floor_multiplier <= 0.0 {
            self.noise_floor_multiplier = defaults.noise_floor_multiplier;
        }
    }

    pub fn config_dir() -> PathBuf {
        // Prefer %APPDATA%\AutoDuck for writability
        if let Some(data_dir) = dirs::data_dir() {
            let app_dir = data_dir.join("AutoDuck");
            if app_dir.exists() || std::fs::create_dir_all(&app_dir).is_ok() {
                // Migrate config from old location (exe directory) if new location has no config
                let new_config = app_dir.join("config.toml");
                if !new_config.exists() {
                    if let Some(exe_dir) = std::env::current_exe()
                        .ok()
                        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                    {
                        let old_config = exe_dir.join("config.toml");
                        if old_config.exists() && std::fs::copy(&old_config, &new_config).is_ok() {
                            // 验证新文件可正确解析后才删除旧文件
                            if let Ok(content) = std::fs::read_to_string(&new_config) {
                                if toml::from_str::<AppConfig>(&content).is_ok() {
                                    let _ = std::fs::remove_file(&old_config);
                                } else {
                                    // 新文件损坏，保留旧文件
                                    let _ = std::fs::remove_file(&new_config);
                                }
                            }
                        }
                    }
                }
                return app_dir;
            }
        }
        // Fallback to exe directory
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
