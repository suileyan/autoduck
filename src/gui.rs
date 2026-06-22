use crate::audio_capture::VAD_FRAME_DURATION_MS;
use crate::config::{validate_process_name, AppConfig, DuckMode};
use crate::hotkey::{format_hotkey, vk_to_code};
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use raw_window_handle::HasWindowHandle;
use slint::Model;
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
use std::sync::atomic::AtomicPtr;
use std::sync::Arc;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT,
    VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetDesktopWindow, GetForegroundWindow, GetMessageW,
    PeekMessageW, PostThreadMessageW, SetForegroundWindow, ShowWindow, TranslateMessage,
    PM_NOREMOVE, SW_HIDE, SW_SHOW, WH_KEYBOARD_LL, MSG,
};
use windows::Win32::System::Threading::GetCurrentThreadId;

slint::include_modules!();

/// Message sent from GUI to main loop
#[derive(Debug, Clone)]
pub enum GuiMessage {
    ConfigChanged(AppConfig),
    RefreshApps,
    HotkeyChanged(String),
    SuspendHotkey(String),
    RestoreHotkey(String),
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

/// 从 AppConfig 设置窗口属性
fn apply_config_to_window(win: &SettingsWindow, config: &AppConfig) {
    win.set_duck_mode(match config.duck_mode {
        DuckMode::Global => "global".into(),
        DuckMode::Apps => "apps".into(),
    });
    win.set_duck_ratio(config.duck_ratio);
    win.set_vad_threshold(config.vad_threshold);
    win.set_attack_frames(config.attack_frames as i32);
    win.set_release_frames(config.release_frames as i32);
    win.set_attack_ms((config.attack_frames * VAD_FRAME_DURATION_MS).to_string().into());
    win.set_release_ms((config.release_frames * VAD_FRAME_DURATION_MS).to_string().into());
    win.set_duck_duration_ms(config.duck_duration_ms as i32);
    win.set_restore_duration_ms(config.restore_duration_ms as i32);
    win.set_spectral_flatness_threshold(config.spectral_flatness_threshold);
    win.set_noise_floor_multiplier(config.noise_floor_multiplier);
    win.set_hotkey(config.hotkey.clone().into());
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
        apply_config_to_window(&window, config);

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
            if let Some(win) = win_apply.upgrade() {
                let config = AppConfig::from_window(&win);
                let _ = sender_apply.send(GuiMessage::ConfigChanged(config));
                win.set_status_text("设置已应用".into());
            }
        });

        // --- Callback: Reset Settings ---
        let win_reset = window.as_weak();
        window.on_reset_settings(move || {
            if let Some(win) = win_reset.upgrade() {
                let default = AppConfig::default();
                apply_config_to_window(&win, &default);
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
            }
        });

        // --- Callback: Add Excluded App ---
        let win_add = window.as_weak();
        window.on_add_excluded_app(move |name: slint::SharedString| {
            let name_str = name.to_string();
            if !validate_process_name(&name_str) {
                if let Some(win) = win_add.upgrade() {
                    win.set_status_text("无效的进程名".into());
                }
                return;
            }
            let name_str = name_str.to_string();
            if let Some(win) = win_add.upgrade() {
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
            }
        });

        // --- Callback: Remove Excluded App ---
        let win_remove = window.as_weak();
        window.on_remove_excluded_app(move |name: slint::SharedString| {
            let name_str = name.to_string();
            if let Some(win) = win_remove.upgrade() {
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
            }
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
                let frames = (ms / VAD_FRAME_DURATION_MS).max(1);
                if let Some(win) = win_attack.upgrade() {
                    win.set_attack_frames(frames as i32);
                }
            }
        });

        // --- Callback: Release ms changed ---
        let win_release = window.as_weak();
        window.on_release_ms_changed(move |val: slint::SharedString| {
            if let Ok(ms) = val.parse::<u32>() {
                let frames = (ms / VAD_FRAME_DURATION_MS).max(1);
                if let Some(win) = win_release.upgrade() {
                    win.set_release_frames(frames as i32);
                }
            }
        });

        // --- Callback: Capture hotkey ---
        // 启动独立线程运行 WH_KEYBOARD_LL 钩子捕获按键
        let win_hotkey = window.as_weak();
        let sender_capture = sender.clone();
        window.on_capture_hotkey(move || {
            dbg_output("[hotkey-capture] on_capture_hotkey 回调触发，启动钩子线程");

            // 进入捕获模式前，暂停 RegisterHotKey 以避免其拦截按键
            let current_hotkey = if let Some(win) = win_hotkey.upgrade() {
                win.get_hotkey().to_string()
            } else {
                String::new()
            };
            let _ = sender_capture.send(GuiMessage::SuspendHotkey(current_hotkey));

            // 将焦点转移到桌面，使设置窗口失去键盘焦点但保持可见
            // 这样 WH_KEYBOARD_LL 钩子才能正常捕获按键
            if let Some(win) = win_hotkey.upgrade() {
                if get_hwnd(&win).is_some() {
                    unsafe {
                        let _ = SetForegroundWindow(GetDesktopWindow());
                    }
                    dbg_output("[hotkey-capture] 已将焦点转移到桌面");
                }
            }

            let (tx, rx) = crossbeam_channel::bounded(1);

            // 启动钩子线程
            std::thread::spawn(move || {
                dbg_output("[hotkey-capture] 钩子线程已启动");
                run_keyboard_hook(tx);
                dbg_output("[hotkey-capture] 钩子线程已退出");
            });

            // 用 slint::Timer 轮询 channel
            let win = win_hotkey.clone();
            let sender_timer = sender_capture.clone();
            let capture_timer = slint::Timer::default();
            capture_timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(50), move || {
                // 排空 channel，避免快速按键时丢失中间状态
                loop {
                    match rx.try_recv() {
                        Ok(result) => {
                            dbg_output(&format!("[hotkey-capture] Timer 收到结果: {:?}", result));
                            if let Some(win) = win.upgrade() {
                                match result {
                                    CaptureResult::Key { vk, ctrl, alt, shift } => {
                                        dbg_output(&format!("[hotkey-capture] 按键: vk=0x{:02X}, ctrl={}, alt={}, shift={}", vk, ctrl, alt, shift));
                                        if let Some(code) = vk_to_code(VIRTUAL_KEY(vk)) {
                                            let mut modifiers = global_hotkey::hotkey::Modifiers::empty();
                                            if ctrl { modifiers |= global_hotkey::hotkey::Modifiers::CONTROL; }
                                            if alt { modifiers |= global_hotkey::hotkey::Modifiers::ALT; }
                                            if shift { modifiers |= global_hotkey::hotkey::Modifiers::SHIFT; }
                                            let hotkey_str = format_hotkey(modifiers, code);
                                            dbg_output(&format!("[hotkey-capture] 成功捕获快捷键: {}", hotkey_str));
                                            win.set_hotkey(hotkey_str.clone().into());
                                            win.set_hotkey_capturing(false);
                                            // 恢复 RegisterHotKey（使用新快捷键）
                                            let _ = sender_timer.send(GuiMessage::RestoreHotkey(hotkey_str));
                                            return; // 捕获完成，退出本次 Timer 回调
                                        } else {
                                            dbg_output(&format!("[hotkey-capture] vk=0x{:02X} 不支持，继续等待", vk));
                                        }
                                    }
                                    CaptureResult::Cancel => {
                                        dbg_output("[hotkey-capture] Esc 取消捕获");
                                        let before = win.get_hotkey_before_capture().to_string();
                                        win.set_hotkey(before.clone().into());
                                        win.set_hotkey_capturing(false);
                                        // 恢复 RegisterHotKey（使用原快捷键）
                                        let _ = sender_timer.send(GuiMessage::RestoreHotkey(before));
                                        return; // 捕获取消，退出本次 Timer 回调
                                    }
                                }
                            }
                        }
                        Err(crossbeam_channel::TryRecvError::Empty) => {
                            // channel 已空，检查窗口状态
                            break; // 跳出循环，在下方检查焦点
                        }
                        Err(crossbeam_channel::TryRecvError::Disconnected) => {
                            dbg_output("[hotkey-capture] 钩子线程 channel 已断开");
                            return;
                        }
                    }
                }
                // channel 为空时检查窗口焦点状态
                if let Some(win) = win.upgrade() {
                    // 检查是否已退出捕获模式（如用户再次点击取消）
                    if !win.get_hotkey_capturing() {
                        dbg_output("[hotkey-capture] 检测到已退出捕获模式，停止钩子");
                        stop_keyboard_hook();
                        // 恢复 RegisterHotKey（使用当前快捷键）
                        let hotkey = win.get_hotkey().to_string();
                        let _ = sender_timer.send(GuiMessage::RestoreHotkey(hotkey));
                    } else if let Some(hwnd) = get_hwnd(&win) {
                        // 检测窗口是否重新获得焦点（用户点击了回来）
                        // 此时若快捷键未被更改，视为取消捕获
                        let foreground = unsafe { GetForegroundWindow() };
                        if foreground == hwnd {
                            dbg_output("[hotkey-capture] 窗口重新获得焦点，取消捕获");
                            let before = win.get_hotkey_before_capture().to_string();
                            win.set_hotkey(before.clone().into());
                            win.set_hotkey_capturing(false);
                            stop_keyboard_hook();
                            let _ = sender_timer.send(GuiMessage::RestoreHotkey(before));
                        }
                    }
                }
            });

            // 保持 Timer 存活
            CAPTURE_TIMER.with(|t| {
                *t.borrow_mut() = Some(capture_timer);
            });
        });

        // --- Callback: Apply hotkey ---
        let win_apply_hotkey = window.as_weak();
        let sender_apply_hotkey = sender.clone();
        window.on_apply_hotkey(move || {
            if let Some(win) = win_apply_hotkey.upgrade() {
                let hotkey = win.get_hotkey().to_string();
                let _ = sender_apply_hotkey.send(GuiMessage::HotkeyChanged(hotkey));
                win.set_status_text("快捷键已应用".into());
            }
        });

        // --- Callback: Reset hotkey ---
        let win_reset_hotkey = window.as_weak();
        let sender_reset_hotkey = sender.clone();
        window.on_reset_hotkey(move || {
            if let Some(win) = win_reset_hotkey.upgrade() {
                win.set_hotkey("Ctrl+Shift+D".into());
                let _ = sender_reset_hotkey.send(GuiMessage::HotkeyChanged("Ctrl+Shift+D".into()));
            }
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
                        apply_config_to_window(&win, &config);
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

// ─── 快捷键捕获：独立线程 WH_KEYBOARD_LL 钩子 ───────────────────────

/// 钩子线程的线程 ID，用于 PostThreadMessageW 发送 WM_QUIT 停止钩子
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);

/// 钩子回调使用的全局 sender 指针
static HOOK_SENDER_PTR: AtomicPtr<Sender<CaptureResult>> = AtomicPtr::new(std::ptr::null_mut());

// 保持捕获定时器存活的 thread-local 存储
thread_local! {
    static CAPTURE_TIMER: std::cell::RefCell<Option<slint::Timer>> = const { std::cell::RefCell::new(None) };
}

/// 钩子回调发送到 GUI 线程的按键结果
#[derive(Debug)]
enum CaptureResult {
    /// 非修饰键按下
    Key { vk: u16, ctrl: bool, alt: bool, shift: bool },
    /// Esc 取消
    Cancel,
}

/// 在独立线程上运行 WH_KEYBOARD_LL 钩子
fn run_keyboard_hook(tx: Sender<CaptureResult>) {
    unsafe {
        let module = GetModuleHandleW(None).unwrap_or_default();
        dbg_output("[hotkey-capture] 正在安装 WH_KEYBOARD_LL 钩子...");
        // 确保消息队列在安装钩子前已创建
        let mut peek_msg = MSG::default();
        let _ = PeekMessageW(&mut peek_msg, None, 0, 0, PM_NOREMOVE);
        dbg_output("[hotkey-capture] 消息队列已创建");
        let hook = match windows::Win32::UI::WindowsAndMessaging::SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_hook_proc),
            Some(module.into()),
            0,
        ) {
            Ok(h) => {
                dbg_output("[hotkey-capture] 钩子安装成功");
                h
            }
            Err(e) => {
                dbg_output(&format!("[hotkey-capture] 钩子安装失败: {:?}", e));
                return;
            }
        };

        // 保存线程 ID，以便 GUI 线程可以停止钩子
        let thread_id = GetCurrentThreadId();
        HOOK_THREAD_ID.store(thread_id, AtomicOrdering::SeqCst);
        dbg_output(&format!("[hotkey-capture] 钩子线程 ID: {}", thread_id));

        // 保存 sender 到全局，供钩子回调使用
        HOOK_SENDER_PTR.store(Arc::into_raw(Arc::new(tx)) as *mut _, AtomicOrdering::SeqCst);

        // 运行 Win32 消息循环——这是 WH_KEYBOARD_LL 钩子正常工作的必要条件
        dbg_output("[hotkey-capture] 进入消息循环，等待按键...");
        let mut msg = MSG::default();
        loop {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 == -1 {
                dbg_output("[hotkey-capture] GetMessageW 返回错误，退出消息循环");
                break;
            }
            if ret.0 == 0 {
                dbg_output("[hotkey-capture] 收到 WM_QUIT，退出消息循环");
                break;
            }
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
        }
        dbg_output("[hotkey-capture] 消息循环已退出");

        // 清理：卸载钩子
        let _ = windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(hook);
        HOOK_THREAD_ID.store(0, AtomicOrdering::SeqCst);
        dbg_output("[hotkey-capture] 钩子已卸载");

        // 释放 sender
        let sender_ptr = HOOK_SENDER_PTR.swap(std::ptr::null_mut(), AtomicOrdering::SeqCst);
        if !sender_ptr.is_null() {
            let _ = Arc::from_raw(sender_ptr as *const _);
        }
    }
}

/// 停止钩子线程（从 GUI 线程调用）
fn stop_keyboard_hook() {
    // 原子地将 thread_id 置 0，同时获取旧值
    // swap 保证只有一方拿到非 0 thread_id，不会重复发送 WM_QUIT
    let thread_id = HOOK_THREAD_ID.swap(0, AtomicOrdering::SeqCst);
    if thread_id != 0 {
        unsafe {
            let _ = PostThreadMessageW(
                thread_id,
                windows::Win32::UI::WindowsAndMessaging::WM_QUIT,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
    // 清理 HOOK_SENDER_PTR，防止钩子线程异常终止时内存泄漏
    let sender_ptr = HOOK_SENDER_PTR.swap(std::ptr::null_mut(), AtomicOrdering::SeqCst);
    if !sender_ptr.is_null() {
        let _ = unsafe { Arc::from_raw(sender_ptr as *const _) };
    }
}

/// WH_KEYBOARD_LL 钩子回调
unsafe extern "system" fn keyboard_hook_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    dbg_output(&format!("[hotkey-capture] hook_proc called: n_code={}", n_code));
    if n_code >= 0 {
        // WM_KEYDOWN = 0x0100, WM_SYSKEYDOWN = 0x0104
        let msg_type = w_param.0 as u32;
        if msg_type == 0x0100 || msg_type == 0x0104 {
            if l_param.0 == 0 {
                return CallNextHookEx(None, n_code, w_param, l_param);
            }
            let kb_struct = &*(l_param.0 as *const windows::Win32::UI::WindowsAndMessaging::KBDLLHOOKSTRUCT);
            let vk = kb_struct.vkCode as u16;
            dbg_output(&format!("[hotkey-capture] 钩子回调: msg=0x{:04X}, vk=0x{:02X}", msg_type, vk));

            // Esc 取消捕获
            if vk == 0x1b {
                dbg_output("[hotkey-capture] 检测到 Esc，取消捕获");
                if let Some(sender) = get_hook_sender() {
                    let _ = sender.send(CaptureResult::Cancel);
                }
                let tid = HOOK_THREAD_ID.load(AtomicOrdering::SeqCst);
                if tid != 0 {
                    let _ = PostThreadMessageW(tid, windows::Win32::UI::WindowsAndMessaging::WM_QUIT, WPARAM(0), LPARAM(0));
                }
                return LRESULT(1);
            }

            // 忽略单独的修饰键
            if is_modifier_key(vk) {
                dbg_output(&format!("[hotkey-capture] 修饰键 vk=0x{:02X}，忽略", vk));
                return CallNextHookEx(None, n_code, w_param, l_param);
            }

            // 读取当前修饰键状态
            let ctrl = is_key_pressed(VK_CONTROL) || is_key_pressed(VK_LCONTROL) || is_key_pressed(VK_RCONTROL);
            let alt = is_key_pressed(VK_MENU) || is_key_pressed(VK_LMENU) || is_key_pressed(VK_RMENU);
            let shift = is_key_pressed(VK_SHIFT) || is_key_pressed(VK_LSHIFT) || is_key_pressed(VK_RSHIFT);

            dbg_output(&format!("[hotkey-capture] 发送按键: vk=0x{:02X}, ctrl={}, alt={}, shift={}", vk, ctrl, alt, shift));
            if let Some(sender) = get_hook_sender() {
                let _ = sender.send(CaptureResult::Key { vk, ctrl, alt, shift });
            }

            let tid = HOOK_THREAD_ID.load(AtomicOrdering::SeqCst);
            if tid != 0 {
                let _ = PostThreadMessageW(tid, windows::Win32::UI::WindowsAndMessaging::WM_QUIT, WPARAM(0), LPARAM(0));
            }
            return LRESULT(1);
        }
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}

fn get_hook_sender() -> Option<Arc<Sender<CaptureResult>>> {
    let ptr = HOOK_SENDER_PTR.load(AtomicOrdering::SeqCst);
    if ptr.is_null() {
        None
    } else {
        // SAFETY: ptr was created by Arc::into_raw and is still valid
        let arc: Arc<Sender<CaptureResult>> = unsafe { Arc::from_raw(ptr) };
        // Put the raw pointer back so it remains valid for other callers
        let _ = Arc::into_raw(arc.clone());
        Some(arc)
    }
}

/// 安全的调试输出，委托到 main.rs 中的共享实现
fn dbg_output(s: &str) {
    crate::dbg_output(s)
}

fn is_modifier_key(vk: u16) -> bool {
    matches!(
        vk,
        0x10 | 0xA0 | 0xA1 | // VK_SHIFT, VK_LSHIFT, VK_RSHIFT
        0x11 | 0xA2 | 0xA3 | // VK_CONTROL, VK_LCONTROL, VK_RCONTROL
        0x12 | 0xA4 | 0xA5    // VK_MENU, VK_LMENU, VK_RMENU
    )
}

fn is_key_pressed(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) < 0 }
}
