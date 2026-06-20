use crate::config::{validate_process_name, AppConfig, DuckMode};
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use slint::Model;

slint::include_modules!();

/// Message sent from GUI to main loop
#[derive(Debug, Clone)]
pub enum GuiMessage {
    ConfigChanged(AppConfig),
    RefreshApps,
}

/// Message sent from main loop to GUI
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum GuiUpdate {
    AppList(Vec<(String, bool)>),
    ConfigReset(AppConfig),
    ShowSettings,
    Quit,
}

pub struct GuiApp {
    window: SettingsWindow,
}

impl GuiApp {
    pub fn new(
        config: &AppConfig,
        sender: Sender<GuiMessage>,
        update_rx: Receiver<GuiUpdate>,
    ) -> Result<Self> {
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
        window.set_spectral_flatness_threshold(config.spectral_flatness_threshold);
        window.set_noise_floor_multiplier(config.noise_floor_multiplier);

        // Set initial excluded apps
        let app_entries: Vec<AppEntry> = config
            .excluded_apps
            .iter()
            .map(|name| AppEntry {
                name: name.into(),
                excluded: true,
            })
            .collect();
        let app_model = std::rc::Rc::new(slint::VecModel::from(app_entries));
        window.set_app_list(app_model.clone().into());

        // --- Callback: Apply Settings ---
        let win_apply = window.as_weak();
        let sender_apply = sender.clone();
        window.on_apply_settings(move || {
            let win = win_apply.upgrade().unwrap();
            let config = AppConfig::from_window(&win);
            let _ = sender_apply.send(GuiMessage::ConfigChanged(config));
            win.set_status_text("设置已应用".into());
        });

        // --- Callback: Reset Settings ---
        let win_reset = window.as_weak();
        window.on_reset_settings(move || {
            let default = AppConfig::default();
            let win = win_reset.upgrade().unwrap();
            win.set_duck_mode(match default.duck_mode {
                DuckMode::Global => "global".into(),
                DuckMode::Apps => "apps".into(),
            });
            win.set_duck_ratio(default.duck_ratio);
            win.set_vad_threshold(default.vad_threshold);
            win.set_attack_frames(default.attack_frames as i32);
            win.set_release_frames(default.release_frames as i32);
            win.set_duck_duration_ms(default.duck_duration_ms as i32);
            win.set_restore_duration_ms(default.restore_duration_ms as i32);
            win.set_spectral_flatness_threshold(default.spectral_flatness_threshold);
            win.set_noise_floor_multiplier(default.noise_floor_multiplier);

            // Reset the app list to default excluded apps
            let entries: Vec<AppEntry> = default
                .excluded_apps
                .iter()
                .map(|name| AppEntry {
                    name: name.into(),
                    excluded: true,
                })
                .collect();
            win.set_app_list(std::rc::Rc::new(slint::VecModel::from(entries)).into());
            win.set_status_text("已重置为默认值".into());
        });

        // --- Callback: Add Excluded App ---
        let win_add = window.as_weak();
        window.on_add_excluded_app(move |name: slint::SharedString| {
            let name_str = name.to_string();
            if !validate_process_name(&name_str) {
                let win = win_add.upgrade().unwrap();
                win.set_status_text("无效的进程名".into());
                return;
            }
            let upper_name = name_str.to_uppercase();
            let win = win_add.upgrade().unwrap();
            let model = win.get_app_list();
            let vec_model = model.as_any()
                .downcast_ref::<slint::VecModel<AppEntry>>()
                .unwrap();

            // Check if already in the list
            for i in 0..vec_model.row_count() {
                if vec_model.row_data(i).map(|e| e.name.to_string().to_uppercase() == upper_name).unwrap_or(false) {
                    // Already exists, just mark as excluded
                    if let Some(mut entry) = vec_model.row_data(i) {
                        entry.excluded = true;
                        vec_model.set_row_data(i, entry);
                    }
                    win.set_status_text("".into());
                    return;
                }
            }

            // Not in list, add new entry
            vec_model.push(AppEntry {
                name: upper_name.into(),
                excluded: true,
            });
            win.set_status_text("".into());
        });

        // --- Callback: Remove Excluded App ---
        let win_remove = window.as_weak();
        window.on_remove_excluded_app(move |name: slint::SharedString| {
            let upper_name = name.to_string().to_uppercase();
            let win = win_remove.upgrade().unwrap();
            let model = win.get_app_list();
            let vec_model = model.as_any()
                .downcast_ref::<slint::VecModel<AppEntry>>()
                .unwrap();

            for i in 0..vec_model.row_count() {
                if vec_model.row_data(i).map(|e| e.name.to_string().to_uppercase() == upper_name).unwrap_or(false) {
                    if let Some(mut entry) = vec_model.row_data(i) {
                        entry.excluded = false;
                        vec_model.set_row_data(i, entry);
                    }
                    break;
                }
            }
            win.set_status_text("".into());
        });

        // --- Callback: Refresh Apps ---
        let sender_refresh = sender;
        window.on_refresh_apps(move || {
            let _ = sender_refresh.send(GuiMessage::RefreshApps);
        });

        // --- Intercept window close: hide instead of destroying ---
        // This keeps the window alive (just hidden) so it can be shown again.
        window.window().on_close_requested(|| slint::CloseRequestResponse::HideWindow);

        // --- Timer to poll for updates from main loop ---
        let win_timer = window.as_weak();
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(100), move || {
            let win = match win_timer.upgrade() {
                Some(w) => w,
                None => return,
            };
            while let Ok(update) = update_rx.try_recv() {
                match update {
                    GuiUpdate::AppList(apps) => {
                        let entries: Vec<AppEntry> = apps
                            .into_iter()
                            .map(|(name, excluded)| AppEntry {
                                name: name.into(),
                                excluded,
                            })
                            .collect();
                        win.set_app_list(std::rc::Rc::new(slint::VecModel::from(entries)).into());
                    }
                    GuiUpdate::ConfigReset(config) => {
                        win.set_duck_mode(match config.duck_mode {
                            DuckMode::Global => "global".into(),
                            DuckMode::Apps => "apps".into(),
                        });
                        win.set_duck_ratio(config.duck_ratio);
                        win.set_vad_threshold(config.vad_threshold);
                        win.set_attack_frames(config.attack_frames as i32);
                        win.set_release_frames(config.release_frames as i32);
                        win.set_duck_duration_ms(config.duck_duration_ms as i32);
                        win.set_restore_duration_ms(config.restore_duration_ms as i32);
                        win.set_spectral_flatness_threshold(config.spectral_flatness_threshold);
                        win.set_noise_floor_multiplier(config.noise_floor_multiplier);
                    }
                    GuiUpdate::ShowSettings => {
                        let _ = win.show();
                    }
                    GuiUpdate::Quit => {
                        let _ = slint::quit_event_loop();
                        return;
                    }
                }
            }
        });

        Ok(Self { window })
    }

    #[allow(dead_code)]
    pub fn run(&self) {
        self.window.run().unwrap();
    }

    pub fn show(&self) {
        let _ = self.window.show();
    }

    #[allow(dead_code)]
    pub fn hide(&self) {
        self.window.hide().unwrap();
    }
}

/// Helper to construct AppConfig from current window state
impl AppConfig {
    fn from_window(win: &SettingsWindow) -> Self {
        let duck_mode_str = win.get_duck_mode().to_string();
        let duck_mode = if duck_mode_str == "apps" {
            DuckMode::Apps
        } else {
            DuckMode::Global
        };

        let mut excluded_apps = Vec::new();
        let model = win.get_app_list();
        if let Some(vec_model) = model.as_any().downcast_ref::<slint::VecModel<AppEntry>>() {
            for i in 0..vec_model.row_count() {
                if let Some(entry) = vec_model.row_data(i) {
                    if entry.excluded {
                        excluded_apps.push(entry.name.to_string().to_uppercase());
                    }
                }
            }
        }

        Self {
            duck_mode,
            duck_ratio: win.get_duck_ratio(),
            excluded_apps,
            vad_threshold: win.get_vad_threshold(),
            attack_frames: win.get_attack_frames() as u32,
            release_frames: win.get_release_frames() as u32,
            duck_duration_ms: win.get_duck_duration_ms() as u32,
            restore_duration_ms: win.get_restore_duration_ms() as u32,
            spectral_flatness_threshold: win.get_spectral_flatness_threshold(),
            noise_floor_multiplier: win.get_noise_floor_multiplier(),
        }
    }
}
