use crate::config::{validate_process_name, AppConfig, DuckMode};
use anyhow::Result;
use crossbeam_channel::Sender;

slint::include_modules!();

/// Message sent from GUI to main loop
#[derive(Debug, Clone)]
pub enum GuiMessage {
    ConfigChanged(AppConfig),
    RefreshApps,
}

pub struct GuiApp {
    window: SettingsWindow,
}

impl GuiApp {
    pub fn new(config: &AppConfig, sender: Sender<GuiMessage>) -> Result<Self> {
        let window = SettingsWindow::new()?;

        // Set initial values from config
        window.set_duck_mode(match config.duck_mode {
            DuckMode::Global => "global".into(),
            DuckMode::Apps => "apps".into(),
        });
        window.set_duck_ratio(config.duck_ratio);
        window.set_vad_threshold(config.vad_threshold);
        window.set_attack_frames(config.attack_frames as i32);
        window.set_release_frames(config.release_frames as i32);
        window.set_duck_duration_ms(config.duck_duration_ms as i32);
        window.set_restore_duration_ms(config.restore_duration_ms as i32);

        // Set initial excluded apps
        let app_entries: Vec<AppEntry> = config
            .excluded_apps
            .iter()
            .map(|name| AppEntry {
                name: name.into(),
                excluded: true,
            })
            .collect();
        window.set_app_list(std::rc::Rc::new(slint::VecModel::from(app_entries)).into());

        // Connect callbacks
        let sender_apply = sender.clone();
        window.on_apply_settings(move || {
            let _ = sender_apply.send(GuiMessage::RefreshApps); // placeholder
        });

        let sender_reset = sender.clone();
        window.on_reset_settings(move || {
            let default_config = AppConfig::default();
            let _ = sender_reset.send(GuiMessage::ConfigChanged(default_config));
        });

        let sender_add = sender.clone();
        window.on_add_excluded_app(move |name: slint::SharedString| {
            let name_str = name.to_string();
            if validate_process_name(&name_str) {
                let mut cfg = AppConfig::default();
                cfg.excluded_apps.push(name_str.to_uppercase());
                let _ = sender_add.send(GuiMessage::ConfigChanged(cfg));
            }
        });

        let sender_remove = sender.clone();
        window.on_remove_excluded_app(move |name: slint::SharedString| {
            let name_str = name.to_string();
            let mut cfg = AppConfig::default();
            cfg.excluded_apps.retain(|app| app != &name_str.to_uppercase());
            let _ = sender_remove.send(GuiMessage::ConfigChanged(cfg));
        });

        let sender_refresh = sender;
        window.on_refresh_apps(move || {
            let _ = sender_refresh.send(GuiMessage::RefreshApps);
        });

        Ok(Self { window })
    }

    pub fn run(&self) {
        self.window.run().unwrap();
    }

    pub fn show(&self) {
        self.window.show().unwrap();
    }

    pub fn hide(&self) {
        self.window.hide().unwrap();
    }

    pub fn update_app_list(&self, apps: Vec<(String, bool)>) {
        let entries: Vec<AppEntry> = apps
            .into_iter()
            .map(|(name, excluded)| AppEntry {
                name: name.into(),
                excluded,
            })
            .collect();
        self.window.set_app_list(std::rc::Rc::new(slint::VecModel::from(entries)).into());
    }

    pub fn update_from_config(&self, config: &AppConfig) {
        self.window.set_duck_mode(match config.duck_mode {
            DuckMode::Global => "global".into(),
            DuckMode::Apps => "apps".into(),
        });
        self.window.set_duck_ratio(config.duck_ratio);
        self.window.set_vad_threshold(config.vad_threshold);
        self.window.set_attack_frames(config.attack_frames as i32);
        self.window.set_release_frames(config.release_frames as i32);
        self.window.set_duck_duration_ms(config.duck_duration_ms as i32);
        self.window.set_restore_duration_ms(config.restore_duration_ms as i32);
    }
}
