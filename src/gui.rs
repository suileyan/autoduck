use crate::config::{validate_process_name, AppConfig, DuckMode};
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use raw_window_handle::HasWindowHandle;
use slint::Model;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOW};

slint::include_modules!();

/// Message sent from GUI to main loop
#[derive(Debug, Clone)]
pub enum GuiMessage {
    ConfigChanged(AppConfig),
    RefreshApps,
    HotkeyChanged(String),
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
    _timer: slint::Timer,
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
        window.set_attack_ms((config.attack_frames * 16).to_string().into());
        window.set_release_ms((config.release_frames * 16).to_string().into());
        window.set_duck_duration_ms(config.duck_duration_ms as i32);
        window.set_restore_duration_ms(config.restore_duration_ms as i32);
        window.set_spectral_flatness_threshold(config.spectral_flatness_threshold);
        window.set_noise_floor_multiplier(config.noise_floor_multiplier);
        window.set_hotkey(config.hotkey.clone().into());

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
            win.set_attack_ms((default.attack_frames * 16).to_string().into());
            win.set_release_ms((default.release_frames * 16).to_string().into());
            win.set_duck_duration_ms(default.duck_duration_ms as i32);
            win.set_restore_duration_ms(default.restore_duration_ms as i32);
            win.set_spectral_flatness_threshold(default.spectral_flatness_threshold);
            win.set_noise_floor_multiplier(default.noise_floor_multiplier);
            win.set_hotkey(default.hotkey.clone().into());
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
            let name_str = name_str.to_string();
            let win = win_add.upgrade().unwrap();
            let model = win.get_app_list();
            let vec_model = model.as_any()
                .downcast_ref::<slint::VecModel<AppEntry>>()
                .unwrap();

            // Check if already in the list (case-insensitive)
            for i in 0..vec_model.row_count() {
                if vec_model.row_data(i).map(|e| e.name.to_string().eq_ignore_ascii_case(&name_str)).unwrap_or(false) {
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
                name: name_str.into(),
                excluded: true,
            });
            win.set_status_text("".into());
        });

        // --- Callback: Remove Excluded App ---
        let win_remove = window.as_weak();
        window.on_remove_excluded_app(move |name: slint::SharedString| {
            let name_str = name.to_string();
            let win = win_remove.upgrade().unwrap();
            let model = win.get_app_list();
            let vec_model = model.as_any()
                .downcast_ref::<slint::VecModel<AppEntry>>()
                .unwrap();

            for i in 0..vec_model.row_count() {
                if vec_model.row_data(i).map(|e| e.name.to_string().eq_ignore_ascii_case(&name_str)).unwrap_or(false) {
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
        let sender_refresh = sender.clone();
        window.on_refresh_apps(move || {
            let _ = sender_refresh.send(GuiMessage::RefreshApps);
        });

        // --- Callback: Attack ms changed ---
        let win_attack = window.as_weak();
        window.on_attack_ms_changed(move |val: slint::SharedString| {
            if let Ok(ms) = val.parse::<u32>() {
                let frames = (ms / 16).max(1);
                if let Some(win) = win_attack.upgrade() {
                    win.set_attack_frames(frames as i32);
                }
            }
        });

        // --- Callback: Release ms changed ---
        let win_release = window.as_weak();
        window.on_release_ms_changed(move |val: slint::SharedString| {
            if let Ok(ms) = val.parse::<u32>() {
                let frames = (ms / 16).max(1);
                if let Some(win) = win_release.upgrade() {
                    win.set_release_frames(frames as i32);
                }
            }
        });

        // --- Callback: Hotkey changed ---
        let sender_hotkey = sender.clone();
        window.on_hotkey_changed(move |val: slint::SharedString| {
            let _ = sender_hotkey.send(GuiMessage::HotkeyChanged(val.to_string()));
        });

        // --- Intercept window close: use Win32 SW_HIDE instead of slint hide() ---
        // slint's Window::hide() calls quit_event_loop() when window_count reaches 0,
        // which kills the event loop and prevents reopening. We use KeepWindowShown
        // to prevent slint from calling hide(), then use Win32 ShowWindow(SW_HIDE)
        // to visually hide the window (disappears from taskbar).
        let win_close = window.as_weak();
        window.window().on_close_requested(move || {
            if let Some(win) = win_close.upgrade() {
                if let Some(hwnd) = get_hwnd(&win) {
                    unsafe {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                }
                // Fallback: if HWND not available, just keep window shown
                // (slint will handle hiding via its own mechanism)
            }
            slint::CloseRequestResponse::KeepWindowShown
        });

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
                        win.set_attack_ms((config.attack_frames * 16).to_string().into());
                        win.set_release_ms((config.release_frames * 16).to_string().into());
                        win.set_duck_duration_ms(config.duck_duration_ms as i32);
                        win.set_restore_duration_ms(config.restore_duration_ms as i32);
                        win.set_spectral_flatness_threshold(config.spectral_flatness_threshold);
                        win.set_noise_floor_multiplier(config.noise_floor_multiplier);
                        win.set_hotkey(config.hotkey.clone().into());
                    }
                    GuiUpdate::ShowSettings => {
                        // Show the window using Win32 ShowWindow(SW_SHOW)
                        // This bypasses slint's show() which would re-increment window_count
                        if let Some(hwnd) = get_hwnd(&win) {
                            unsafe {
                                let _ = ShowWindow(hwnd, SW_SHOW);
                            }
                        } else {
                            // Fallback: use slint's show() if Win32 HWND not available
                            let _ = win.show();
                        }
                    }
                    GuiUpdate::Quit => {
                        let _ = slint::quit_event_loop();
                        return;
                    }
                }
            }
        });

        Ok(Self { window, _timer: timer })
    }

    pub fn show(&self) {
        let _ = self.window.show();
    }
}

/// Extract the Win32 HWND from a slint SettingsWindow
fn get_hwnd(win: &SettingsWindow) -> Option<HWND> {
    let slint_handle = win.window().window_handle();
    let raw = slint_handle.window_handle().ok()?;
    match raw.as_raw() {
        raw_window_handle::RawWindowHandle::Win32(win32_handle) => {
            let ptr = win32_handle.hwnd.get() as *mut core::ffi::c_void;
            Some(HWND(ptr))
        }
        _ => None,
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
                        excluded_apps.push(entry.name.to_string());
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
            enabled: true, // enabled 由托盘/快捷键控制，不从窗口读取
            hotkey: win.get_hotkey().to_string(),
        }
    }
}
