use crate::config::DuckMode;
use anyhow::Result;
use crossbeam_channel::Sender;
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
const ID_MODE_GLOBAL: &str = "mode_global";
const ID_MODE_APPS: &str = "mode_apps";
const ID_AUTO_START: &str = "auto_start";
const ID_SETTINGS: &str = "settings";
const ID_QUIT: &str = "quit";

#[derive(Debug, Clone)]
pub enum TrayEvent {
    Quit,
    ToggleMode(DuckMode),
    ToggleAutoStart(bool),
    OpenSettings,
}

#[allow(dead_code)]
pub struct TrayApp {
    tray_icon: TrayIcon,
    event_sender: Sender<TrayEvent>,
    current_mode: DuckMode,
    auto_start_enabled: bool,
    crashed: bool,
}

#[allow(dead_code)]
impl TrayApp {
    pub fn new(
        event_sender: Sender<TrayEvent>,
        mode: DuckMode,
        auto_start: bool,
    ) -> Result<Self> {
        // Create a simple 16x16 blue icon
        let rgba = vec![0u8, 120, 215, 255].repeat(16 * 16);
        let icon = Icon::from_rgba(rgba, 16, 16)?;

        let menu = Self::build_menu_inner(mode, auto_start);

        let tray_icon = TrayIconBuilder::new()
            .with_tooltip("AutoDuck")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()?;

        Ok(Self {
            tray_icon,
            event_sender,
            current_mode: mode,
            auto_start_enabled: auto_start,
            crashed: false,
        })
    }

    pub fn build_menu(&self) -> Menu {
        Self::build_menu_inner(self.current_mode, self.auto_start_enabled)
    }

    fn build_menu_inner(mode: DuckMode, auto_start: bool) -> Menu {
        let mode_global = CheckMenuItemBuilder::new()
            .id(MenuId::new(ID_MODE_GLOBAL))
            .text("全局降音")
            .enabled(true)
            .checked(mode == DuckMode::Global)
            .build();

        let mode_apps = CheckMenuItemBuilder::new()
            .id(MenuId::new(ID_MODE_APPS))
            .text("应用排除")
            .enabled(true)
            .checked(mode == DuckMode::Apps)
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
            true,
            &[
                &mode_global,
                &PredefinedMenuItem::separator(),
                &mode_apps,
            ],
        )
        .expect("failed to create mode submenu");

        let menu = Menu::new();
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

    pub fn update_mode(&mut self, mode: DuckMode) {
        self.current_mode = mode;
        self.rebuild_menu();
    }

    pub fn update_auto_start(&mut self, enabled: bool) {
        self.auto_start_enabled = enabled;
        self.rebuild_menu();
    }

    fn handle_menu_event(&self, event: muda::MenuEvent) {
        if event.id == MenuId::new(ID_MODE_GLOBAL) {
            let _ = self.event_sender.send(TrayEvent::ToggleMode(DuckMode::Global));
        } else if event.id == MenuId::new(ID_MODE_APPS) {
            let _ = self.event_sender.send(TrayEvent::ToggleMode(DuckMode::Apps));
        } else if event.id == MenuId::new(ID_AUTO_START) {
            let _ = self
                .event_sender
                .send(TrayEvent::ToggleAutoStart(!self.auto_start_enabled));
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
    running: Arc<AtomicBool>,
) -> Result<()> {
    let app = TrayApp::new(event_sender, mode, auto_start)?;

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

        // Small sleep to avoid busy-waiting
        std::thread::sleep(Duration::from_millis(10));
    }

    Ok(())
}
