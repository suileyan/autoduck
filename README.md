# AutoDuck

麦克风人声自动降音量系统 — 检测到麦克风人声时自动压低系统音量，说话结束后恢复。纯本地运行，托盘后台进程，Release 构建无控制台窗口。

## 功能特性

- **神经网络 VAD**：基于 [earshot](https://github.com/pykeio/earshot) 纯 Rust 实现的语音活动检测，有效过滤键盘声和风扇声
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
- **无控制台窗口**：Release 构建不显示控制台黑窗口

## 系统要求

- Windows 10/11 (x64)
- 麦克风设备

## 下载安装

从 [Releases](https://github.com/suileyan/autoduck/releases) 页面下载最新版本的 zip 文件，解压后运行 `autoduck.exe` 即可。

## 使用方法

1. 运行 `autoduck.exe`
2. 程序以托盘图标后台运行（无控制台窗口）
3. 右键托盘图标可：
   - 切换降音模式（全局 / 应用排除）
   - 开关开机自启
   - 打开设置窗口
   - 退出程序
4. 在设置窗口中可调整所有参数，点击"应用"即时生效

## 配置文件

配置文件位于 `%APPDATA%\AutoDuck\config.toml`，首次运行自动生成。

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
attack_frames = 4

# 进入 Silent 状态所需连续静音帧数 (每帧 16ms)
release_frames = 30

# 降音渐变时长 (ms)，控制从当前音量降到目标音量的过渡速度
duck_duration_ms = 120

# 恢复渐变时长 (ms)，控制从降音恢复到原始音量的过渡速度
restore_duration_ms = 120
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

## 架构

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
| VAD | 重采样 → earshot 检测 → 状态机 | 无 |
| 音量控制 | 接收状态变更 → 执行降音/恢复 | `COINIT_MULTITHREADED` |
| 托盘 | win32 消息循环 + 菜单事件 | 无 |
| GUI | slint 设置窗口事件循环 | 无 |

### 音频处理链路

```
cpal 原生格式 (48kHz/44.1kHz, 单/双声道)
  → 多声道降混为单声道
  → 重采样到 16kHz (整数倍: 抽取+低通, 非整数倍: rubato)
  → 按 256 采样点切帧 (16ms)
  → earshot VAD 检测
  → 防抖状态机
```

## 已知限制

- 默认播放设备热切换后需重启程序（MVP 阶段未处理 `IMMNotificationClient`）
- 独占模式音频客户端可能导致音量调用被忽略

## 开发

### 环境要求

- Rust 1.95.0+
- Windows SDK

### 构建

```bash
cargo build --release
```

### 运行测试

```bash
cargo test
```

## 技术栈

- [earshot](https://github.com/pykeio/earshot) — 纯 Rust 神经网络 VAD
- [cpal](https://github.com/RustAudio/cpal) — 跨平台音频 I/O
- [rubato](https://github.com/HEnquist/rubato) — 音频重采样
- [rtrb](https://github.com/crossbeam-rs/rtrb) — 无锁环形缓冲区
- [windows-rs](https://github.com/microsoft/windows-rs) — Windows API 绑定
- [slint](https://slint.dev) — 声明式 GUI 框架
- [tray-icon](https://github.com/tauri-apps/tray-icon) — 系统托盘图标
- [muda](https://github.com/tauri-apps/muda) — 原生菜单

## License

MIT
