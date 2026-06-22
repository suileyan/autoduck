# AutoDuck

**[English](#english)** | **[中文](#中文)**

---

<a id="中文"></a>

麦克风人声自动降音量系统 — 检测到麦克风人声时自动压低系统音量，说话结束后恢复。纯本地运行，托盘后台进程，Release 构建无控制台窗口。

### 功能特性

- **神经网络 VAD**：基于 [earshot](https://github.com/pykeio/earshot) 纯 Rust 实现的语音活动检测，有效过滤键盘声和风扇声
- **频谱平坦度预过滤**：自动识别白噪声/风扇声等平坦频谱，避免误触发
- **自适应噪声底**：EMA 跟踪环境噪声，自动适配不同使用场景
- **防抖状态机**：连续多帧确认后才触发状态切换，避免瞬态噪音误触发和说话停顿处频繁回弹
- **双模式音量控制**：
  - **全局模式（默认）**：检测到人声时压低系统主音量，开箱即用
  - **应用排除模式**：只压低非排除列表应用的音量，保留 Teams、微信等通讯软件原音量
- **GUI 设置窗口**：可视化配置所有参数，无需手动编辑配置文件
- **应用排除选择器**：从当前音频会话中一键选择要排除的应用，也可手动输入进程名
- **可配置降音/恢复渐变时长**：独立调整降音和恢复的渐变速度，从 50ms 到 500ms
- **音量渐变**：平滑过渡，避免瞬间跳变
- **高 DPI 支持**：Per-Monitor V2 DPI 感知，在高分屏上界面清晰
- **系统托盘**：右键菜单切换模式、开关自启、打开设置、退出
- **单实例锁**：防止多个进程争抢音量控制
- **崩溃可观测**：工作线程 panic 时自动恢复音量并提示
- **配置持久化**：TOML 格式配置文件，首次运行自动生成，原子写入防损坏
- **开机自启**：注册表方式，托盘菜单一键开关
- **全局快捷键**：可自定义快捷键一键切换启用/禁用，GUI 中支持按键捕获设置

### 系统要求

- Windows 10/11 (x64)
- 麦克风设备

### 下载安装

从 [Releases](https://github.com/suileyan/autoduck/releases) 页面下载最新版本的 zip 文件，解压后运行 `autoduck.exe` 即可。

### 使用方法

1. 运行 `autoduck.exe`
2. 程序以托盘图标后台运行（无控制台窗口）
3. 右键托盘图标可：
   - 切换降音模式（全局 / 应用排除）
   - 开关开机自启
   - 打开设置窗口
   - 退出程序
4. 在设置窗口中可调整所有参数，点击"应用"即时生效

### 配置文件

配置文件位于 `%APPDATA%\AutoDuck\config.toml`，首次运行自动生成。若 exe 同目录下存在旧版配置文件，会自动迁移到新路径。

```toml
# 降音模式："global" (全局主音量) 或 "apps" (应用级排除)
duck_mode = "global"

# 降音比例 (0.0 - 1.0)，0.3 表示压低到 30% 音量
duck_ratio = 0.3

# 当 duck_mode = "apps" 时生效，这些应用的音量不会被压低
excluded_apps = ["Teams.exe", "WeChat.exe", "OUTLOOK.EXE", "WINWORD.EXE"]

# VAD 语音检测阈值 (0.0 - 1.0)
vad_threshold = 0.5

# 进入 Speaking 状态所需连续语音帧数 (每帧 16ms)
attack_frames = 6

# 进入 Silent 状态所需连续静音帧数 (每帧 16ms)
release_frames = 30

# 降音渐变时长 (ms)
duck_duration_ms = 120

# 恢复渐变时长 (ms)
restore_duration_ms = 120

# 频谱平坦度阈值 (0.0 - 1.0)，高于此值的声音被视为噪声
spectral_flatness_threshold = 0.65

# 噪声底倍率 (1.0 - 5.0)，有效阈值 = max(用户阈值, 噪声底 × 倍率)
noise_floor_multiplier = 2.0

# 是否启用降音 (true/false)
enabled = true

# 全局快捷键，按下切换启用/禁用
hotkey = "Ctrl+Shift+D"
```

### 参数调优建议

| 参数 | 建议范围 | 说明 |
|---|---|---|
| `vad_threshold` | 0.3 - 0.7 | 值越高越不容易被噪音触发，但可能漏检轻声说话 |
| `attack_frames` | 3 - 8 | 值越大对噪音越不敏感，但响应延迟增加（每帧 16ms） |
| `release_frames` | 20 - 50 | 值越大在说话停顿处越不会回弹音量，但恢复延迟增加 |
| `duck_ratio` | 0.1 - 0.5 | 值越小降音幅度越大 |
| `duck_duration_ms` | 50 - 500 | 降音渐变时长，值越小降音越快，但可能听感突兀 |
| `restore_duration_ms` | 50 - 500 | 恢复渐变时长，值越大恢复越平滑 |
| `spectral_flatness_threshold` | 0.5 - 0.8 | 值越低过滤越严格，更多声音被当作噪声 |
| `noise_floor_multiplier` | 1.5 - 3.0 | 值越大对环境噪声越不敏感 |

### 架构

```
┌─────────────┐    rtrb     ┌──────────────┐   VoiceState   ┌──────────────────┐
│  cpal 音频   │───────────▶│  VAD 线程     │──────────────▶│  音量控制线程      │
│  回调线程    │  ring buffer│  earshot +   │  crossbeam     │  模式A: 全局      │
│  (采集+降混) │            │  防抖状态机   │  channel       │  模式B: 应用排除  │
└─────────────┘             └──────────────┘                └──────────────────┘
                                                                  │
┌─────────────┐    TrayEvent                                   │
│  托盘线程    │────────────────────────────────────────────────▶│  主事件循环
│  win32 消息  │                                                │  (main.rs)
│  + 快捷键    │                                                │
└─────────────┘                                                │
                                                               │
┌─────────────┐    GuiMessage                                  │
│  GUI 线程    │────────────────────────────────────────────────▶│
│  slint 设置  │                                                │
│  窗口        │◀── GuiUpdate ──────────────────────────────────│
└─────────────┘
```

### 线程模型

| 线程 | 职责 | COM 初始化 |
|---|---|---|
| 音频回调 | 采集 + 降混 + 写入 ring buffer | 无 |
| VAD | 重采样 → 频谱平坦度 → earshot → 状态机 | 无 |
| 音量控制 | 接收状态变更 → 执行降音/恢复 | `COINIT_MULTITHREADED` |
| 托盘 | win32 消息循环 + 菜单事件 + 全局快捷键 | 无 |
| GUI | slint 设置窗口事件循环 | 无 |

### 音频处理链路

```
cpal 原生格式 (48kHz/44.1kHz, 单/双声道)
  → 多声道降混为单声道
  → 重采样到 16kHz (整数倍: 抽取+低通, 非整数倍: rubato)
  → 按 256 采样点切帧 (16ms)
  → 频谱平坦度预过滤 (Hann窗 + FFT)
  → earshot VAD 检测
  → 自适应噪声底有效阈值
  → 防抖状态机
```

### 已知限制

- 默认播放设备热切换后需重启程序（MVP 阶段未处理 `IMMNotificationClient`）
- 独占模式音频客户端可能导致音量调用被忽略

### 开发

#### 环境要求

- Rust 1.87+
- Windows SDK

#### 构建

```bash
cargo build --release
```

#### 运行测试

```bash
cargo test
```

### 技术栈

- [earshot](https://github.com/pykeio/earshot) — 纯 Rust 神经网络 VAD
- [cpal](https://github.com/RustAudio/cpal) — 跨平台音频 I/O
- [rubato](https://github.com/HEnquist/rubato) — 音频重采样
- [rtrb](https://github.com/crossbeam-rs/rtrb) — 无锁环形缓冲区
- [rustfft](https://github.com/razorpy/rustfft) — FFT 频谱分析
- [windows-rs](https://github.com/microsoft/windows-rs) — Windows API 绑定
- [slint](https://slint.dev) — 声明式 GUI 框架
- [tray-icon](https://github.com/tauri-apps/tray-icon) — 系统托盘图标
- [muda](https://github.com/tauri-apps/muda) — 原生菜单

### License

MIT

---

<a id="english"></a>

Auto ducking system — automatically lowers system volume when speech is detected on the microphone, and restores it when you stop talking. Runs entirely locally as a system tray background process with no console window in Release builds.

### Features

- **Neural Network VAD**: Speech activity detection powered by [earshot](https://github.com/pykeio/earshot), a pure Rust neural network implementation that effectively filters out keyboard clicks and fan noise
- **Spectral Flatness Pre-filter**: Automatically identifies flat-spectrum noise (white noise, fans) to prevent false triggers
- **Adaptive Noise Floor**: EMA-based noise tracking that automatically adapts to different environments
- **Debounced State Machine**: Requires consecutive frame confirmation before triggering state transitions, avoiding false triggers from transient noise and volume bouncing during speech pauses
- **Dual Volume Control Modes**:
  - **Global Mode (default)**: Lowers the system master volume when speech is detected — works out of the box
  - **App Exclusion Mode**: Only lowers volume for non-excluded apps, keeping communication tools like Teams and WeChat at original volume
- **GUI Settings Window**: Visual configuration for all parameters — no manual config file editing needed
- **App Exclusion Selector**: One-click selection from current audio sessions, or manually enter process names
- **Configurable Duck/Restore Fade Duration**: Independently adjust duck and restore fade speed from 50ms to 500ms
- **Smooth Volume Transitions**: Gradual volume changes to avoid jarring jumps
- **High DPI Support**: Per-Monitor V2 DPI awareness for crisp UI on high-resolution displays
- **System Tray**: Right-click menu to switch modes, toggle auto-start, open settings, or quit
- **Single Instance Lock**: Prevents multiple processes from fighting over volume control
- **Crash Observability**: Automatically restores volume and notifies when worker threads panic
- **Persistent Configuration**: TOML config file auto-generated on first run, with atomic writes to prevent corruption
- **Auto-Start on Boot**: Registry-based, toggleable from the tray menu
- **Global Hotkey**: Customizable hotkey to toggle enable/disable, with key capture in the GUI settings
- **No Console Window**: Release builds don't show a console window

### System Requirements

- Windows 10/11 (x64)
- Microphone device

### Download & Install

Download the latest zip file from the [Releases](https://github.com/suileyan/autoduck/releases) page, extract it, and run `autoduck.exe`.

### Usage

1. Run `autoduck.exe`
2. The program runs in the background as a tray icon (no console window)
3. Right-click the tray icon to:
   - Switch ducking mode (Global / App Exclusion)
   - Toggle auto-start on boot
   - Open the settings window
   - Quit the program
4. Adjust parameters in the settings window and click "Apply" for instant effect

### Configuration File

The configuration file is located at `%APPDATA%\AutoDuck\config.toml`, auto-generated on first run. If a legacy config file exists in the executable's directory, it will be automatically migrated to the new path.

```toml
# Ducking mode: "global" (master volume) or "apps" (per-app exclusion)
duck_mode = "global"

# Duck ratio (0.0 - 1.0), 0.3 means lower to 30% volume
duck_ratio = 0.3

# When duck_mode = "apps", these apps' volume will NOT be lowered
excluded_apps = ["Teams.exe", "WeChat.exe", "OUTLOOK.EXE", "WINWORD.EXE"]

# VAD speech detection threshold (0.0 - 1.0)
vad_threshold = 0.5

# Consecutive speech frames required to enter Speaking state (each frame is 16ms)
attack_frames = 6

# Consecutive silence frames required to enter Silent state (each frame is 16ms)
release_frames = 30

# Duck fade duration (ms)
duck_duration_ms = 120

# Restore fade duration (ms)
restore_duration_ms = 120

# Spectral flatness threshold (0.0 - 1.0), sounds above this are treated as noise
spectral_flatness_threshold = 0.65

# Noise floor multiplier (1.0 - 5.0), effective threshold = max(user threshold, noise floor × multiplier)
noise_floor_multiplier = 2.0

# Whether ducking is enabled (true/false)
enabled = true

# Global hotkey, press to toggle enable/disable
hotkey = "Ctrl+Shift+D"
```

### Tuning Guide

| Parameter | Suggested Range | Description |
|---|---|---|
| `vad_threshold` | 0.3 - 0.7 | Higher values reduce noise triggers but may miss quiet speech |
| `attack_frames` | 3 - 8 | Higher values reduce noise sensitivity but increase response delay (16ms per frame) |
| `release_frames` | 20 - 50 | Higher values prevent volume bouncing during speech pauses but increase recovery delay |
| `duck_ratio` | 0.1 - 0.5 | Lower values mean more aggressive volume reduction |
| `duck_duration_ms` | 50 - 500 | Duck fade duration; lower values mean faster ducking but may sound abrupt |
| `restore_duration_ms` | 50 - 500 | Restore fade duration; higher values mean smoother recovery |
| `spectral_flatness_threshold` | 0.5 - 0.8 | Lower values are stricter — more sounds are treated as noise |
| `noise_floor_multiplier` | 1.5 - 3.0 | Higher values reduce sensitivity to ambient noise |

### Architecture

```
┌─────────────┐    rtrb     ┌──────────────┐   VoiceState   ┌──────────────────┐
│  cpal audio  │───────────▶│  VAD thread   │──────────────▶│  Volume thread    │
│  callback    │  ring buffer│  earshot +   │  crossbeam     │  Mode A: Global   │
│  (capture +  │            │  state machine│  channel       │  Mode B: App excl.│
│   downmix)   │            └──────────────┘                └──────────────────┘
└─────────────┘                                                  │
                                                                  │
┌─────────────┐    TrayEvent                                   │
│  Tray thread │────────────────────────────────────────────────▶│  Main event loop
│  win32 msg   │                                                │  (main.rs)
│  + hotkey    │                                                │
└─────────────┘                                                │
                                                               │
┌─────────────┐    GuiMessage                                  │
│  GUI thread  │────────────────────────────────────────────────▶│
│  slint       │                                                │
│  settings    │◀── GuiUpdate ──────────────────────────────────│
└─────────────┘
```

### Thread Model

| Thread | Responsibility | COM Init |
|---|---|---|
| Audio callback | Capture + downmix + write to ring buffer | None |
| VAD | Resample → spectral flatness → earshot → state machine | None |
| Volume control | Receive state changes → execute duck/restore | `COINIT_MULTITHREADED` |
| Tray | Win32 message loop + menu events + global hotkey | None |
| GUI | Slint settings window event loop | None |

### Audio Processing Pipeline

```
cpal native format (48kHz/44.1kHz, mono/stereo)
  → Multi-channel downmix to mono
  → Resample to 16kHz (integer ratio: decimation+lowpass, non-integer: rubato)
  → Frame slicing at 256 samples (16ms)
  → Spectral flatness pre-filter (Hann window + FFT)
  → earshot VAD detection
  → Adaptive noise floor effective threshold
  → Debounced state machine
```

### Known Limitations

- Default audio device hot-swap requires program restart (`IMMNotificationClient` not handled in MVP)
- Exclusive-mode audio clients may cause volume API calls to be ignored

### Development

#### Prerequisites

- Rust 1.87+
- Windows SDK

#### Build

```bash
cargo build --release
```

#### Run Tests

```bash
cargo test
```

### Tech Stack

- [earshot](https://github.com/pykeio/earshot) — Pure Rust neural network VAD
- [cpal](https://github.com/RustAudio/cpal) — Cross-platform audio I/O
- [rubato](https://github.com/HEnquist/rubato) — Audio resampling
- [rtrb](https://github.com/crossbeam-rs/rtrb) — Lock-free ring buffer
- [rustfft](https://github.com/razorpy/rustfft) — FFT spectral analysis
- [windows-rs](https://github.com/microsoft/windows-rs) — Windows API bindings
- [slint](https://slint.dev) — Declarative GUI framework
- [tray-icon](https://github.com/tauri-apps/tray-icon) — System tray icon
- [muda](https://github.com/tauri-apps/muda) — Native menu

### License

MIT
