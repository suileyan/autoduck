#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio_capture;
mod autostart;
mod config;
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
use tray_icon::TrayEvent;
use vad_state::{VadStateMachine, VoiceState};
use volume_control::VolumeController;
use volume_worker::{VolumeCommand, VolumeWorker};

fn main() -> anyhow::Result<()> {
    // 1. 单实例锁
    let _single_instance = single_instance::SingleInstance::new()?;

    // 2. 加载配置
    let config_path = AppConfig::config_file_path();
    let config = AppConfig::load(&config_path)?;

    // 3. 创建通道
    let (volume_cmd_tx, volume_cmd_rx) = unbounded::<VolumeCommand>();
    let (tray_event_tx, tray_event_rx) = unbounded::<TrayEvent>();
    let (vad_state_tx, vad_state_rx) = bounded::<VoiceState>(4);
    let (crash_tx, crash_rx) = bounded::<String>(2);

    // 4. 启动音量控制线程
    let volume_controller = VolumeController::new(config.duck_mode, config.excluded_apps.clone(), config.duck_duration_ms, config.restore_duration_ms)?;
    let duck_ratio = config.duck_ratio;
    let volume_worker = VolumeWorker::new(volume_controller, volume_cmd_rx, duck_ratio);
    let volume_cmd_tx_clone = volume_cmd_tx.clone();
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

    let vad_handle = std::thread::Builder::new()
        .name("vad-worker".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_vad_loop(
                    running_clone,
                    vad_threshold,
                    attack_frames,
                    release_frames,
                    vad_state_tx,
                    volume_cmd_tx_clone,
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
    let tray_handle = std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || {
            if let Err(e) = tray_icon::run_tray(tray_event_tx_clone, tray_mode, auto_start) {
                eprintln!("托盘线程错误: {}", e);
            }
        })?;

    // 7. 主事件循环：监听 VAD 状态变化、托盘事件、崩溃通知
    let mut current_voice_state = VoiceState::Silent;

    loop {
        // 检查崩溃通知
        if let Ok(crash_msg) = crash_rx.try_recv() {
            eprintln!("工作线程崩溃: {}", crash_msg);
            // 退出前先恢复音量
            let _ = volume_cmd_tx.send(VolumeCommand::Restore);
            std::thread::sleep(std::time::Duration::from_millis(200));
            break;
        }

        // 处理 VAD 状态变化
        if let Ok(new_state) = vad_state_rx.try_recv() {
            if new_state != current_voice_state {
                current_voice_state = new_state;
                match new_state {
                    VoiceState::Speaking => {
                        let _ = volume_cmd_tx.send(VolumeCommand::Duck);
                    }
                    VoiceState::Silent => {
                        let _ = volume_cmd_tx.send(VolumeCommand::Restore);
                    }
                }
            }
        }

        // 处理托盘事件
        if let Ok(event) = tray_event_rx.try_recv() {
            match event {
                TrayEvent::Quit => {
                    // 退出前先恢复音量
                    let _ = volume_cmd_tx.send(VolumeCommand::Restore);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    running.store(false, Ordering::Relaxed);
                    let _ = volume_cmd_tx.send(VolumeCommand::Stop);
                    break;
                }
                TrayEvent::ToggleMode(mode) => {
                    // TODO: 重新创建 VolumeController 并重启音量控制线程
                    eprintln!("切换降音模式为: {:?}（需要重启生效）", mode);
                }
                TrayEvent::ToggleAutoStart(enable) => {
                    if enable {
                        if let Err(e) = autostart::enable_auto_start() {
                            eprintln!("启用开机自启失败: {}", e);
                        }
                    } else {
                        if let Err(e) = autostart::disable_auto_start() {
                            eprintln!("禁用开机自启失败: {}", e);
                        }
                    }
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // 等待线程退出
    running.store(false, Ordering::Relaxed);
    let _ = volume_cmd_tx.send(VolumeCommand::Stop);
    let _ = vad_handle.join();
    let _ = volume_handle.join();
    let _ = tray_handle.join();

    Ok(())
}

fn run_vad_loop(
    running: Arc<AtomicBool>,
    threshold: f32,
    attack_frames: u32,
    release_frames: u32,
    vad_state_tx: Sender<VoiceState>,
    _volume_cmd_tx: Sender<VolumeCommand>,
) {
    let (producer, consumer) = audio_capture::AudioCapture::create_ring_buffer();

    let mut capture = audio_capture::AudioCapture::new();
    if let Err(e) = capture.start(producer) {
        eprintln!("音频采集启动失败: {}", e);
        return;
    }

    // 获取实际采样率用于 FrameReader
    // 使用默认 48000Hz，因为 cpal 通常使用这个采样率
    let native_sample_rate = 48000;
    let mut frame_reader = audio_capture::FrameReader::new(consumer, native_sample_rate);

    let mut detector = Detector::default();
    let mut state_machine = VadStateMachine::new(attack_frames, release_frames);

    while running.load(Ordering::Relaxed) {
        if let Some(frame) = frame_reader.next_frame() {
            // earshot 需要精确 256 个 i16 采样
            let frame_i16: Vec<i16> = frame
                .iter()
                .map(|&s| (s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16)
                .collect();

            if frame_i16.len() == 256 {
                let score = detector.predict_i16(&frame_i16);
                if let Some(new_state) = state_machine.update(score, threshold) {
                    let _ = vad_state_tx.send(new_state);
                }
            }
        } else {
            // 没有足够的数据，短暂等待
            std::thread::sleep(std::time::Duration::from_millis(4));
        }
    }

    capture.stop();
}
