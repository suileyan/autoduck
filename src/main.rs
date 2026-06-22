#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio_capture;
mod autostart;
mod config;
mod gui;
mod hotkey;
mod single_instance;
mod tray_icon;
mod vad_state;
mod volume_control;
mod volume_worker;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crossbeam_channel::{bounded, unbounded, Sender};
use earshot::Detector;

use config::AppConfig;
use config::DuckMode;
use gui::{GuiApp, GuiMessage, GuiUpdate};
use tray_icon::{TrayEvent, TrayUpdate};
use vad_state::{VadStateMachine, VoiceState, NoiseFloorTracker, SpectralFlatnessState};
use rustfft::FftPlanner;
use volume_control::VolumeController;
use volume_worker::{VolumeCommand, VolumeWorker};

enum VadCommand {
    UpdateParams {
        threshold: f32,
        attack_frames: u32,
        release_frames: u32,
        spectral_flatness_threshold: f32,
        noise_floor_multiplier: f32,
    },
    SetEnabled(bool),
}

/// Parameters for the VAD loop, grouped to reduce function argument count.
struct VadParams {
    /// Voice activity detection score threshold (0.0–1.0). Scores above this are considered speech.
    threshold: f32,
    /// Number of consecutive voice frames required to transition from Silent to Speaking.
    attack_frames: u32,
    /// Number of consecutive silence frames required to transition from Speaking to Silent.
    release_frames: u32,
    /// Spectral flatness threshold (0.0–1.0). Frames with flatness above this are treated as noise.
    spectral_flatness_threshold: f32,
    /// Multiplier applied to the noise floor RMS to compute the effective VAD threshold.
    noise_floor_multiplier: f32,
}

/// 安全的调试输出，使用 OutputDebugStringW
/// 替代 eprintln!，确保在 windows_subsystem = "windows" 模式下仍可查看
pub fn dbg_output(s: &str) {
    use windows::core::PCWSTR;
    use windows::Win32::System::Diagnostics::Debug::OutputDebugStringW;
    let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        OutputDebugStringW(PCWSTR(wide.as_ptr()));
    }
}

fn main() -> anyhow::Result<()> {
    // 1. 单实例锁
    let _single_instance = single_instance::SingleInstance::new()?;

    // 2. 加载配置
    let config_path = AppConfig::config_file_path();
    let mut config = AppConfig::load(&config_path)?;

    // 3. 创建通道
    let (volume_cmd_tx, volume_cmd_rx) = unbounded::<VolumeCommand>();
    let (tray_event_tx, tray_event_rx) = unbounded::<TrayEvent>();
    let (vad_state_tx, vad_state_rx) = bounded::<VoiceState>(4);
    let (crash_tx, crash_rx) = bounded::<String>(2);
    let (gui_msg_tx, gui_msg_rx) = unbounded::<GuiMessage>();
    let (gui_update_tx, gui_update_rx) = unbounded::<GuiUpdate>();
    let (vad_cmd_tx, vad_cmd_rx) = unbounded::<VadCommand>();

    // 4. 启动音量控制线程
    let volume_controller = VolumeController::new(config.duck_mode, config.excluded_apps.clone(), config.duck_duration_ms, config.restore_duration_ms)?;
    let duck_ratio = config.duck_ratio;
    let volume_worker = VolumeWorker::new(volume_controller, volume_cmd_rx, duck_ratio);
    let crash_tx_vad = crash_tx.clone();
    let volume_handle = std::thread::Builder::new()
        .name("volume-worker".into())
        .spawn(move || {
            if let Err(e) = volume_worker.run() {
                let _ = crash_tx.send(format!("音量控制线程: {}", e));
            }
        })?;

    // 5. 启动音频采集 + VAD 线程
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let vad_threshold = config.vad_threshold;
    let attack_frames = config.attack_frames;
    let release_frames = config.release_frames;
    let spectral_flatness_threshold = config.spectral_flatness_threshold;
    let noise_floor_multiplier = config.noise_floor_multiplier;

    let vad_params = VadParams {
        threshold: vad_threshold,
        attack_frames,
        release_frames,
        spectral_flatness_threshold,
        noise_floor_multiplier,
    };

    let vad_handle = std::thread::Builder::new()
        .name("vad-worker".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_vad_loop(
                    running_clone,
                    vad_params,
                    vad_state_tx,
                    vad_cmd_rx,
                    // Clone crash_tx for run_vad_loop (which moves it);
                    // the original crash_tx_vad is used after catch_unwind for panic reporting.
                    crash_tx_vad.clone(),
                );
            }));

            if let Err(payload) = result {
                let msg = if let Some(s) = payload.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = payload.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "VAD 线程未知 panic".to_string()
                };
                let _ = crash_tx_vad.send(format!("VAD 线程: {}", msg));
            }
        })?;

    // 6. 启动托盘线程
    let tray_mode = match config.duck_mode {
        config::DuckMode::Global => DuckMode::Global,
        config::DuckMode::Apps => DuckMode::Apps,
    };
    let auto_start = autostart::is_auto_start_enabled();
    let tray_event_tx_clone = tray_event_tx.clone();
    let running_tray = running.clone();
    let (tray_update_tx, tray_update_rx) = unbounded::<TrayUpdate>();
    let tray_enabled = config.enabled;
    let tray_hotkey = config.hotkey.clone();
    let tray_handle = std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || {
            if let Err(e) = tray_icon::run_tray(tray_event_tx_clone, tray_mode, auto_start, tray_enabled, tray_hotkey, running_tray, tray_update_rx) {
                crate::dbg_output(&format!("托盘线程错误: {}", e));
            }
        })?;

    // 7. 主事件循环：监听 VAD 状态变化、托盘事件、GUI 消息、崩溃通知
    let mut current_voice_state = VoiceState::Silent;
    let mut gui_handle: Option<std::thread::JoinHandle<()>> = None;
    let mut _vad_enabled = config.enabled;

    // 如果配置中 enabled=false，通知 VAD 线程暂停
    if !_vad_enabled {
        let _ = vad_cmd_tx.send(VadCommand::SetEnabled(false));
    }

    loop {
        crossbeam_channel::select! {
            recv(crash_rx) -> msg => {
                let crash_msg = match msg {
                    Ok(m) => m,
                    Err(_) => break,
                };
                crate::dbg_output(&format!("工作线程崩溃: {}", crash_msg));
                // 通知托盘显示崩溃状态
                let _ = tray_update_tx.send(TrayUpdate::Crashed);
                // 退出前先恢复音量，使用 ack 确认完成
                let (ack_tx, ack_rx) = crossbeam_channel::bounded::<()>(1);
                let _ = volume_cmd_tx.send(VolumeCommand::Restore { ack: Some(ack_tx) });
                let _ = ack_rx.recv_timeout(std::time::Duration::from_secs(5));
                break;
            }
            recv(vad_state_rx) -> msg => {
                if let Ok(new_state) = msg {
                    if new_state != current_voice_state {
                        current_voice_state = new_state;
                        match new_state {
                            VoiceState::Speaking => {
                                let _ = volume_cmd_tx.send(VolumeCommand::Duck);
                            }
                            VoiceState::Silent => {
                                let _ = volume_cmd_tx.send(VolumeCommand::Restore { ack: None });
                            }
                        }
                    }
                }
            }
            recv(tray_event_rx) -> msg => {
                let event = match msg {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                match event {
                    TrayEvent::Quit => {
                        let (ack_tx, ack_rx) = crossbeam_channel::bounded::<()>(1);
                        let _ = volume_cmd_tx.send(VolumeCommand::Restore { ack: Some(ack_tx) });
                        let _ = ack_rx.recv_timeout(std::time::Duration::from_secs(5));
                        running.store(false, Ordering::Relaxed);
                        let _ = volume_cmd_tx.send(VolumeCommand::Stop);
                        break;
                    }
                    TrayEvent::ToggleEnabled(enabled) => {
                        _vad_enabled = enabled;
                        config.enabled = enabled;
                        if let Err(e) = config.save(&config_path) {
                            crate::dbg_output(&format!("保存配置失败: {}", e));
                        }
                        if enabled {
                            let _ = vad_cmd_tx.send(VadCommand::SetEnabled(true));
                        } else {
                            let _ = vad_cmd_tx.send(VadCommand::SetEnabled(false));
                            let _ = volume_cmd_tx.send(VolumeCommand::Restore { ack: None });
                        }
                        let _ = tray_update_tx.send(TrayUpdate::EnabledChanged(enabled));
                    }
                    TrayEvent::ToggleMode(mode) => {
                        config.duck_mode = mode;
                        if let Err(e) = config.save(&config_path) {
                            crate::dbg_output(&format!("保存配置失败: {}", e));
                        }
                        let _ = volume_cmd_tx.send(VolumeCommand::UpdateConfig(config.clone()));
                    }
                    TrayEvent::ToggleAutoStart(enable) => {
                        if enable {
                            if let Err(e) = autostart::enable_auto_start() {
                                crate::dbg_output(&format!("启用开机自启失败: {}", e));
                            }
                        } else {
                            if let Err(e) = autostart::disable_auto_start() {
                                crate::dbg_output(&format!("禁用开机自启失败: {}", e));
                            }
                        }
                    }
                    TrayEvent::OpenSettings => {
                        if gui_handle.is_none() {
                            let gui_config = config.clone();
                            let gui_msg_tx = gui_msg_tx.clone();
                            let gui_update_rx = gui_update_rx.clone();
                            let handle = std::thread::Builder::new()
                                .name("gui".into())
                                .spawn(move || {
                                    match GuiApp::new(&gui_config, gui_msg_tx, gui_update_rx) {
                                        Ok(gui) => {
                                            gui.show();
                                            let _ = slint::run_event_loop_until_quit();
                                        }
                                        Err(e) => {
                                            crate::dbg_output(&format!("创建设置窗口失败: {}", e));
                                        }
                                    }
                                });
                            if let Ok(h) = handle {
                                gui_handle = Some(h);
                            }
                        } else {
                            let _ = gui_update_tx.send(GuiUpdate::ShowSettings);
                        }
                    }
                }
            }
            recv(gui_msg_rx) -> msg => {
                let msg = match msg {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                match msg {
                    GuiMessage::ConfigChanged(new_config) => {
                        if let Err(e) = new_config.save(&config_path) {
                            crate::dbg_output(&format!("保存配置失败: {}", e));
                        }
                        config = new_config.clone();
                        let _ = volume_cmd_tx.send(VolumeCommand::UpdateConfig(new_config.clone()));
                        let _ = vad_cmd_tx.send(VadCommand::UpdateParams {
                            threshold: new_config.vad_threshold,
                            attack_frames: new_config.attack_frames,
                            release_frames: new_config.release_frames,
                            spectral_flatness_threshold: new_config.spectral_flatness_threshold,
                            noise_floor_multiplier: new_config.noise_floor_multiplier,
                        });
                    }
                    GuiMessage::RefreshApps => {
                        let session_names = volume_control::enumerate_audio_session_names();
                        let apps: Vec<(String, bool)> = session_names.into_iter().map(|name| {
                            let excluded = config.excluded_apps.iter().any(|excluded| excluded.eq_ignore_ascii_case(&name));
                            (name, excluded)
                        }).collect();
                        let _ = gui_update_tx.send(GuiUpdate::AppList(apps));
                    }
                    GuiMessage::HotkeyChanged(hotkey) => {
                        config.hotkey = hotkey;
                        if let Err(e) = config.save(&config_path) {
                            crate::dbg_output(&format!("保存配置失败: {}", e));
                        }
                        let _ = tray_update_tx.send(TrayUpdate::HotkeyChanged(config.hotkey.clone()));
                    }
                    GuiMessage::SuspendHotkey(hotkey) => {
                        let _ = tray_update_tx.send(TrayUpdate::SuspendHotkey(hotkey));
                    }
                    GuiMessage::RestoreHotkey(hotkey) => {
                        let _ = tray_update_tx.send(TrayUpdate::RestoreHotkey(hotkey));
                    }
                }
            }
            default(std::time::Duration::from_secs(30)) => {
                // 安全网：所有 channel 30 秒内无消息时唤醒，防止永久阻塞
            }
        }
    }

    // 等待线程退出
    running.store(false, Ordering::Relaxed);
    let _ = volume_cmd_tx.send(VolumeCommand::Stop);
    let _ = vad_handle.join();
    let _ = volume_handle.join();
    // 托盘线程通过 running 标志退出
    match tray_handle.join() {
        Ok(()) => {}
        Err(_) => crate::dbg_output("托盘线程 join 失败"),
    }
    // 通知 GUI 线程退出事件循环
    let _ = gui_update_tx.send(GuiUpdate::Quit);
    if let Some(h) = gui_handle {
        let _ = h.join();
    }

    Ok(())
}

fn run_vad_loop(
    running: Arc<AtomicBool>,
    params: VadParams,
    vad_state_tx: Sender<VoiceState>,
    vad_cmd_rx: crossbeam_channel::Receiver<VadCommand>,
    crash_tx: Sender<String>,
) {
    // Use a placeholder sample rate for ring buffer; will be updated after capture.start
    let (producer, consumer) = audio_capture::AudioCapture::create_ring_buffer();

    let mut capture = audio_capture::AudioCapture::new();
    let native_sample_rate = match capture.start(producer) {
        Ok(rate) => rate,
        Err(e) => {
            let msg = format!("音频采集启动失败: {}", e);
            crate::dbg_output(&msg);
            let _ = crash_tx.send(msg);
            return;
        }
    };

    // Recreate ring buffer with correct size and FrameReader with actual sample rate
    // Since the producer was already consumed by start(), we use the consumer as-is
    // but the ring buffer was created with 48000 * 2 capacity which is sufficient
    let mut frame_reader = audio_capture::FrameReader::new(consumer, native_sample_rate);

    let mut detector = Detector::default();
    let mut state_machine = VadStateMachine::new(params.attack_frames, params.release_frames);
    let mut noise_tracker = NoiseFloorTracker::new(0.005);
    let mut current_state = VoiceState::Silent;
    let mut threshold = params.threshold;
    let mut spectral_flatness_threshold = params.spectral_flatness_threshold;
    let mut noise_floor_multiplier = params.noise_floor_multiplier;
    let mut fft_planner = FftPlanner::new();
    let mut sf_state = SpectralFlatnessState::new();
    let mut frame_i16 = vec![0i16; 256];
    let mut enabled = true;

    while running.load(Ordering::Relaxed) {
        // Check for parameter updates
        while let Ok(cmd) = vad_cmd_rx.try_recv() {
            match cmd {
                VadCommand::UpdateParams {
                    threshold: new_threshold,
                    attack_frames,
                    release_frames,
                    spectral_flatness_threshold: new_sf_threshold,
                    noise_floor_multiplier: new_nf_multiplier,
                } => {
                    threshold = new_threshold;
                    state_machine.set_attack_frames(attack_frames);
                    state_machine.set_release_frames(release_frames);
                    spectral_flatness_threshold = new_sf_threshold;
                    noise_floor_multiplier = new_nf_multiplier;
                }
                VadCommand::SetEnabled(new_enabled) => {
                    if !new_enabled && current_state == VoiceState::Speaking {
                        // 禁用前通知主循环当前已静音
                        let _ = vad_state_tx.try_send(VoiceState::Silent);
                        current_state = VoiceState::Silent;
                    }
                    enabled = new_enabled;
                    if enabled {
                        // 重新启用时重置状态机
                        state_machine.reset();
                    }
                }
            }
        }

        if let Some(frame) = frame_reader.next_frame() {
            if !enabled {
                // 禁用时仅排空 ring buffer，跳过检测
                continue;
            }
            // Compute RMS for noise floor tracking
            let rms = (frame.iter().map(|&s| s * s).sum::<f32>() / frame.len() as f32).sqrt();

            // Update noise floor during Silent state
            if current_state == VoiceState::Silent {
                noise_tracker.update(rms);
            }

            // Compute spectral flatness pre-filter
            let sf = sf_state.compute(&frame, &mut fft_planner);

            // earshot 需要精确 256 个 i16 采样
            for (i, &s) in frame.iter().enumerate() {
                if i < frame_i16.len() {
                    frame_i16[i] = (s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                }
            }
            let frame_i16_len = frame.len().min(256);

            if frame_i16_len == 256 {
                let score = if sf > spectral_flatness_threshold {
                    // Flat noise: skip earshot, treat as silence
                    0.0
                } else {
                    detector.predict_i16(&frame_i16)
                };

                // Compute effective threshold with noise floor
                let effective_threshold = noise_tracker.effective_threshold(threshold, noise_floor_multiplier);

                if let Some(new_state) = state_machine.update(score, effective_threshold) {
                    current_state = new_state;
                    let _ = vad_state_tx.try_send(new_state);
                }
            }
        } else {
            // 没有足够的数据，短暂等待
            std::thread::sleep(std::time::Duration::from_millis(4));
        }
    }

    capture.stop();
}
