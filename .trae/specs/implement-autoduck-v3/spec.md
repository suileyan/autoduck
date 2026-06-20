# AutoDuck 麦克风人声自动降音量系统 Spec

## Why

用户在电脑上播放音乐/游戏声音时，一旦对着麦克风说话，听到的声音会盖过自己的语音，造成干扰。需要一个纯本地后台程序，实时检测麦克风人声，自动压低系统音量，说话结束后恢复——开箱即用，无需手动操作。

## What Changes

- 实现基于 `earshot` 纯 Rust 神经网络的 VAD（语音活动检测），替代传统能量检测方案
- 实现带防抖的状态机，避免瞬态噪音误触发和说话停顿处频繁回弹
- 基于 `cpal` 的麦克风音频采集 + 重采样到 16kHz 供 VAD 使用
- 实现双模式音量控制架构：
  - **模式 A（默认）**：全局主音量压低/恢复，通过 `IAudioEndpointVolume`
  - **模式 B**：应用级排除模式，只压低非排除列表应用的音量，通过 `IAudioSessionManager2` 系列接口
- 音量变化使用 100-150ms 渐变，避免瞬间跳变
- 使用 `pguidEventContext` 区分自身操作与外部手动改音量
- 系统托盘后台运行，无主窗口
- 单实例锁防止多进程争抢
- 工作线程崩溃可观测（托盘图标变灰提示）
- TOML 配置文件持久化，支持开机自启注册表写入
- 配置文件包含双模式选择、排除应用列表、VAD 参数等

## Impact

- Affected specs: VAD 检测、音频采集、音量控制、进程管理、配置系统、托盘 UI
- Affected code: 全新项目，无现有代码

---

## ADDED Requirements

### Requirement: VAD 语音活动检测

系统 SHALL 使用 `earshot` crate 进行神经网络 VAD 检测，输入为 16kHz 单声道 PCM 数据，每帧精确 256 采样点（16ms），输出语音概率分数。

#### Scenario: 正常语音检测
- **WHEN** 麦克风采集到人声
- **THEN** earshot 返回的语音概率分数 >= 阈值（默认 0.5）

#### Scenario: 键盘声/风扇声过滤
- **WHEN** 麦克风采集到键盘敲击声或风扇噪音
- **THEN** earshot 返回的语音概率分数 < 阈值，不被误判为语音

### Requirement: VAD 防抖状态机

系统 SHALL 实现防抖状态机，连续检测到 `attack_frames` 帧语音才进入 Speaking 状态，连续检测到 `release_frames` 帧静音才回到 Silent 状态。

#### Scenario: 瞬态噪音不触发
- **WHEN** VAD 检测到少于 `attack_frames`（默认 4-6 帧，约 64-96ms）的连续语音帧
- **THEN** 状态机保持 Silent，不触发降音

#### Scenario: 说话停顿不回弹
- **WHEN** 说话中出现短于 `release_frames`（默认 20-40 帧，约 300-600ms）的静音停顿
- **THEN** 状态机保持 Speaking，不恢复音量

### Requirement: 麦克风音频采集与重采样

系统 SHALL 使用 `cpal` 采集麦克风音频，降混为单声道，重采样到 16kHz，按 256 采样点切帧送入 VAD。整数倍采样率（如 48000Hz）使用简单抽取+低通滤波，非整数倍（如 44100Hz）使用 `rubato`。

#### Scenario: 48kHz 采集设备
- **WHEN** 麦克风以 48000Hz 采集
- **THEN** 3:1 抽取 + 低通滤波降为 16kHz

#### Scenario: 44.1kHz 采集设备
- **WHEN** 麦克风以 44100Hz 采集
- **THEN** 使用 rubato 重采样到 16kHz

### Requirement: 全局主音量压低模式（模式 A，默认）

系统 SHALL 在 `duck_mode = "global"` 时，检测到人声后压低系统主音量（乘以 `duck_ratio`），说话结束后恢复。使用 `IAudioEndpointVolume` 接口。

#### Scenario: 检测到人声降音
- **WHEN** 状态机进入 Speaking 且当前模式为 global
- **THEN** 系统主音量在 100-150ms 内渐变至 `当前音量 × duck_ratio`

#### Scenario: 人声结束恢复
- **WHEN** 状态机进入 Silent 且当前模式为 global
- **THEN** 系统主音量在 100-150ms 内渐变恢复至降音前的音量

#### Scenario: 外部手动改音量
- **WHEN** 用户在降音期间手动调整系统音量
- **THEN** 系统识别为外部操作（通过 `pguidEventContext` GUID 对比），放弃原快照，以当前音量为新基准

### Requirement: 应用级排除音量压低模式（模式 B）

系统 SHALL 在 `duck_mode = "apps"` 时，检测到人声后只压低不在 `excluded_apps` 列表中的应用音量，保留排除列表内应用的原音量。使用 `IAudioSessionManager2` 系列接口。

#### Scenario: 检测到人声降音（排除模式）
- **WHEN** 状态机进入 Speaking 且当前模式为 apps
- **THEN** 遍历所有音频会话，进程名不在 `excluded_apps` 中的会话音量渐变至 `原音量 × duck_ratio`，排除列表内的会话不动

#### Scenario: 人声结束恢复（排除模式）
- **WHEN** 状态机进入 Silent 且当前模式为 apps
- **THEN** 遍历之前被压低的会话，根据快照恢复原音量

#### Scenario: 说话期间新应用开启
- **WHEN** 在 Speaking 状态下有新音频会话出现
- **THEN** 若进程名不在排除列表，立即将其音量压低

#### Scenario: 会话列表动态更新
- **WHEN** 应用随时开关导致音频会话变化
- **THEN** 系统每 2 秒周期性重新枚举会话列表

### Requirement: 音量渐变

系统 SHALL 在所有音量变化操作中使用 100-150ms 渐变（8-10 步线性插值，每步间隔 10-15ms），避免瞬间跳变。

#### Scenario: 降音渐变
- **WHEN** 触发降音操作
- **THEN** 音量在 100-150ms 内分 8-10 步平滑降低

#### Scenario: 恢复渐变
- **WHEN** 触发恢复操作
- **THEN** 音量在 100-150ms 内分 8-10 步平滑恢复

### Requirement: 单实例锁

系统 SHALL 使用 Windows 命名 Mutex 确保同一时间只有一个 AutoDuck 实例运行。

#### Scenario: 已有实例运行
- **WHEN** 用户尝试启动第二个 AutoDuck 实例
- **THEN** 第二个实例检测到 `ERROR_ALREADY_EXISTS`，自动退出

### Requirement: 工作线程崩溃可观测

系统 SHALL 在 VAD 线程或音量控制线程 panic 时，通过 `std::panic::catch_unwind` 捕获并上报，托盘图标变灰提示"已停止工作"。

#### Scenario: VAD 线程崩溃
- **WHEN** VAD 工作线程发生 panic
- **THEN** 托盘图标变灰，提示"已停止工作"

#### Scenario: 音量控制线程崩溃
- **WHEN** 音量控制线程发生 panic
- **THEN** 托盘图标变灰，提示"已停止工作"

### Requirement: 配置持久化

系统 SHALL 使用 TOML 格式配置文件，启动时读取，文件不存在时写入默认值。配置包含：降音模式、降音比例、排除应用列表、VAD 阈值、防抖参数。

#### Scenario: 首次启动无配置文件
- **WHEN** 启动时配置文件不存在
- **THEN** 创建默认配置文件并使用默认值运行

#### Scenario: 配置文件存在
- **WHEN** 启动时配置文件存在
- **THEN** 读取并使用文件中的配置值

### Requirement: 开机自启

系统 SHALL 支持通过托盘菜单开关开机自启，实现方式为写/删注册表 `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`。

#### Scenario: 开启开机自启
- **WHEN** 用户在托盘菜单中勾选"开机自启"
- **THEN** 写入注册表 Run 键值

#### Scenario: 关闭开机自启
- **WHEN** 用户在托盘菜单中取消"开机自启"
- **THEN** 删除注册表 Run 键值

### Requirement: 系统托盘后台运行

系统 SHALL 以托盘图标方式后台运行，无主窗口。托盘菜单提供：模式切换、开机自启开关、退出。

#### Scenario: 右键托盘图标
- **WHEN** 用户右键托盘图标
- **THEN** 显示菜单：切换降音模式、开机自启开关、退出

### Requirement: 线程模型与 COM 初始化

系统 SHALL 在音频回调线程中仅做数据采集和写入 ring buffer；VAD 线程从 ring buffer 读取并处理；音量控制线程使用 `COINIT_MULTITHREADED` 初始化 COM；托盘线程运行 win32 消息循环。

#### Scenario: 音频回调线程
- **WHEN** cpal 音频回调触发
- **THEN** 仅做数据采集和写入 rtrb ring buffer，不做 VAD 或音量操作

#### Scenario: 音量控制线程 COM 初始化
- **WHEN** 音量控制线程启动
- **THEN** 使用 `COINIT_MULTITHREADED` 初始化 COM，避免 STA 死锁风险

## 已知限制（MVP 不处理）

- 默认播放设备热切换会导致模式 A 缓存的 `IAudioEndpointVolume` 失效，需重启程序
- 独占模式音频客户端可能导致音量调用被忽略
- 模式 B 中进程名匹配区分大小写（统一转大写比较）
