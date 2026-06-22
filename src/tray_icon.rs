use crate::config::DuckMode;
use crate::hotkey::parse_hotkey;
use anyhow::Result;
use crossbeam_channel::Sender;
use global_hotkey::{
    hotkey::HotKey,
    GlobalHotKeyManager, GlobalHotKeyEvent, HotKeyState,
};
use muda::{
    CheckMenuItemBuilder, Menu, MenuId, MenuItemBuilder, PredefinedMenuItem, Submenu,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
};

// Fixed menu item IDs for event matching
const ID_ENABLE: &str = "enable";
const ID_MODE_GLOBAL: &str = "mode_global";
const ID_MODE_APPS: &str = "mode_apps";
const ID_AUTO_START: &str = "auto_start";
const ID_SETTINGS: &str = "settings";
const ID_QUIT: &str = "quit";

#[derive(Debug, Clone)]
pub enum TrayEvent {
    Quit,
    ToggleEnabled(bool),
    ToggleMode(DuckMode),
    ToggleAutoStart(bool),
    OpenSettings,
}

#[derive(Debug, Clone)]
pub enum TrayUpdate {
    Crashed,
    EnabledChanged(bool),
    HotkeyChanged(String),
    /// 暂停快捷键注册，携带当前快捷键字符串以便恢复
    SuspendHotkey(String),
    RestoreHotkey(String),
}

pub struct TrayApp {
    tray_icon: TrayIcon,
    event_sender: Sender<TrayEvent>,
    #[allow(dead_code)] // Used by rebuild_menu for future menu updates
    current_mode: DuckMode,
    enabled: bool,
    auto_start_enabled: bool,
    crashed: bool,
    update_rx: crossbeam_channel::Receiver<TrayUpdate>,
    hotkey_manager: GlobalHotKeyManager,
    current_hotkey: Option<HotKey>,
    /// 保存当前快捷键字符串，SuspendHotkey 时备份，RestoreHotkey 消息丢失时可恢复
    suspended_hotkey_str: Option<String>,
    /// SuspendHotkey 的时间戳，用于超时自动恢复
    suspended_at: Option<std::time::Instant>,
}

impl TrayApp {
    pub fn new(
        event_sender: Sender<TrayEvent>,
        mode: DuckMode,
        auto_start: bool,
        enabled: bool,
        hotkey_str: &str,
        update_rx: crossbeam_channel::Receiver<TrayUpdate>,
    ) -> Result<Self> {
        // Load icon from embedded PNG
        let icon_bytes = include_bytes!("../icon.png");
        let decoder = png::Decoder::new(std::io::Cursor::new(icon_bytes));
        let mut reader = decoder.read_info()?;
        let buf_size = reader.output_buffer_size().unwrap_or(0);
        let mut buf = vec![0u8; buf_size];
        let info = reader.next_frame(&mut buf)?;
        let icon = Icon::from_rgba(buf, info.width, info.height)?;

        let menu = Self::build_menu_inner(enabled, mode, auto_start);

        let tray_icon = TrayIconBuilder::new()
            .with_tooltip(if enabled { "AutoDuck" } else { "AutoDuck - 已暂停" })
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()?;

        let hotkey_manager = GlobalHotKeyManager::new()?;
        let mut app = Self {
            tray_icon,
            event_sender,
            current_mode: mode,
            enabled,
            auto_start_enabled: auto_start,
            crashed: false,
            update_rx,
            hotkey_manager,
            current_hotkey: None,
            suspended_hotkey_str: None,
            suspended_at: None,
        };

        // 注册初始快捷键
        app.register_hotkey(hotkey_str)?;

        Ok(app)
    }

    fn register_hotkey(&mut self, hotkey_str: &str) -> Result<()> {
        // 先注销旧快捷键
        if let Some(old) = self.current_hotkey.take() {
            let _ = self.hotkey_manager.unregister(old);
        }

        if hotkey_str.trim().is_empty() {
            return Ok(());
        }

        if let Some((modifiers, code)) = parse_hotkey(hotkey_str) {
            let hotkey = HotKey::new(Some(modifiers), code);
            self.hotkey_manager.register(hotkey)?;
            self.current_hotkey = Some(hotkey);
        }

        // 注册成功后清除挂起备份
        self.suspended_hotkey_str = None;
        self.suspended_at = None;

        Ok(())
    }

    #[allow(dead_code)]
    pub fn build_menu(&self) -> Menu {
        Self::build_menu_inner(self.enabled, self.current_mode, self.auto_start_enabled)
    }

    fn build_menu_inner(enabled: bool, mode: DuckMode, auto_start: bool) -> Menu {
        let enable_item = CheckMenuItemBuilder::new()
            .id(MenuId::new(ID_ENABLE))
            .text("启用降音")
            .enabled(true)
            .checked(enabled)
            .build();

        let mode_global = CheckMenuItemBuilder::new()
            .id(MenuId::new(ID_MODE_GLOBAL))
            .text("全局降音")
            .enabled(enabled)
            .checked(enabled && mode == DuckMode::Global)
            .build();

        let mode_apps = CheckMenuItemBuilder::new()
            .id(MenuId::new(ID_MODE_APPS))
            .text("应用排除")
            .enabled(enabled)
            .checked(enabled && mode == DuckMode::Apps)
            .build();

        let auto_start_item = CheckMenuItemBuilder::new()
            .id(MenuId::new(ID_AUTO_START))
            .text("开机自启")
            .enabled(true)
            .checked(auto_start)
            .build();

        let settings_item = MenuItemBuilder::new()
            .id(MenuId::new(ID_SETTINGS))
            .text("设置")
            .enabled(true)
            .build();

        let quit_item = MenuItemBuilder::new()
            .id(MenuId::new(ID_QUIT))
            .text("退出")
            .enabled(true)
            .build();

        let mode_submenu = Submenu::with_items(
            "降音模式",
            enabled,
            &[
                &mode_global,
                &PredefinedMenuItem::separator(),
                &mode_apps,
            ],
        )
        .expect("failed to create mode submenu");

        let menu = Menu::new();
        menu.append(&enable_item)
            .expect("failed to append enable item");
        menu.append(&mode_submenu)
            .expect("failed to append mode submenu");
        menu.append(&PredefinedMenuItem::separator())
            .expect("failed to append separator");
        menu.append(&auto_start_item)
            .expect("failed to append auto_start item");
        menu.append(&PredefinedMenuItem::separator())
            .expect("failed to append separator");
        menu.append(&settings_item)
            .expect("failed to append settings item");
        menu.append(&quit_item)
            .expect("failed to append quit item");

        menu
    }

    fn rebuild_menu(&self) {
        let menu = self.build_menu();
        self.tray_icon.set_menu(Some(Box::new(menu)));
    }

    pub fn set_crashed(&mut self) {
        self.crashed = true;
        let _ = self.tray_icon.set_tooltip(Some("AutoDuck - 已停止工作"));
    }

    pub fn update_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        let _ = self.tray_icon.set_tooltip(Some(if enabled { "AutoDuck" } else { "AutoDuck - 已暂停" }));
        self.rebuild_menu();
    }

    #[allow(dead_code)]
    pub fn update_mode(&mut self, mode: DuckMode) {
        self.current_mode = mode;
        self.rebuild_menu();
    }

    #[allow(dead_code)]
    pub fn update_auto_start(&mut self, enabled: bool) {
        self.auto_start_enabled = enabled;
        self.rebuild_menu();
    }

    fn handle_menu_event(&mut self, event: muda::MenuEvent) {
        if event.id == MenuId::new(ID_ENABLE) {
            let new_enabled = !self.enabled;
            self.enabled = new_enabled;
            let _ = self.event_sender.send(TrayEvent::ToggleEnabled(new_enabled));
        } else if event.id == MenuId::new(ID_MODE_GLOBAL) {
            let _ = self.event_sender.send(TrayEvent::ToggleMode(DuckMode::Global));
        } else if event.id == MenuId::new(ID_MODE_APPS) {
            let _ = self.event_sender.send(TrayEvent::ToggleMode(DuckMode::Apps));
        } else if event.id == MenuId::new(ID_AUTO_START) {
            let new_enabled = !self.auto_start_enabled;
            self.auto_start_enabled = new_enabled;
            let _ = self
                .event_sender
                .send(TrayEvent::ToggleAutoStart(new_enabled));
        } else if event.id == MenuId::new(ID_SETTINGS) {
            let _ = self.event_sender.send(TrayEvent::OpenSettings);
        } else if event.id == MenuId::new(ID_QUIT) {
            let _ = self.event_sender.send(TrayEvent::Quit);
        }
    }
}

pub fn run_tray(
    event_sender: Sender<TrayEvent>,
    mode: DuckMode,
    auto_start: bool,
    enabled: bool,
    hotkey: String,
    running: Arc<AtomicBool>,
    tray_update_rx: crossbeam_channel::Receiver<TrayUpdate>,
) -> Result<()> {
    let mut app = TrayApp::new(event_sender, mode, auto_start, enabled, &hotkey, tray_update_rx)?;

    while running.load(Ordering::Relaxed) {
        // Process Windows messages
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Check for menu events
        if let Ok(event) = muda::MenuEvent::receiver().try_recv() {
            app.handle_menu_event(event);
        }

        // Check for global hotkey events
        if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state == HotKeyState::Pressed {
                // 快捷键触发时切换启用/禁用状态
                let new_enabled = !app.enabled;
                app.enabled = new_enabled;
                let _ = app.event_sender.send(TrayEvent::ToggleEnabled(new_enabled));
            }
        }

        // Check for tray updates from main loop
        if let Ok(update) = app.update_rx.try_recv() {
            match update {
                TrayUpdate::Crashed => app.set_crashed(),
                TrayUpdate::EnabledChanged(enabled) => app.update_enabled(enabled),
                TrayUpdate::HotkeyChanged(hotkey) => {
                    if let Err(e) = app.register_hotkey(&hotkey) {
                        crate::dbg_output(&format!("注册快捷键失败: {}", e));
                    }
                }
                TrayUpdate::SuspendHotkey(hotkey_str) => {
                    // 保存快捷键字符串，防止 RestoreHotkey 消息丢失后无法恢复
                    app.suspended_hotkey_str = Some(hotkey_str);
                    app.suspended_at = Some(std::time::Instant::now());
                    if let Some(old) = app.current_hotkey.take() {
                        let _ = app.hotkey_manager.unregister(old);
                    }
                }
                TrayUpdate::RestoreHotkey(hotkey) => {
                    if let Err(e) = app.register_hotkey(&hotkey) {
                        crate::dbg_output(&format!("恢复快捷键注册失败: {}", e));
                    }
                }
            }
        }

        // 超时自动恢复：若 SuspendHotkey 后 5 秒未收到 RestoreHotkey，自动恢复快捷键
        if app.current_hotkey.is_none() {
            if let (Some(ref hotkey_str), Some(suspended_at)) = (&app.suspended_hotkey_str, app.suspended_at) {
                if suspended_at.elapsed() > Duration::from_secs(5) {
                    let hotkey = hotkey_str.clone();
                    crate::dbg_output(&format!("[tray] SuspendHotkey 超时，自动恢复快捷键: {}", hotkey));
                    if let Err(e) = app.register_hotkey(&hotkey) {
                        crate::dbg_output(&format!("[tray] 自动恢复快捷键失败: {}", e));
                    }
                }
            }
        }

        // Small sleep to avoid busy-waiting
        std::thread::sleep(Duration::from_millis(10));
    }

    Ok(())
}
