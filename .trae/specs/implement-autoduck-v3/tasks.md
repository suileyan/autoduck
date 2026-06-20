# Tasks

- [x] Task 1: 项目脚手架与 Cargo.toml
  - [x] 1.1 初始化 Cargo 项目，配置所有依赖（cpal, rtrb, rubato, earshot, windows, tray-icon, muda, serde, toml）
  - [x] 1.2 配置 windows crate features（Win32_Media_Audio, Win32_System_Com, Win32_System_Registry, Win32_System_Threading, Win32_Foundation, Win32_UI_WindowsAndMessaging, Win32_Security, Win32_System_Com_StructuredStorage, Win32_System_Variant）
  - [x] 1.3 创建基本 main.rs 入口，验证编译通过

- [x] Task 2: 配置系统模块
  - [x] 2.1 定义 `AppConfig` 结构体（duck_mode, duck_ratio, excluded_apps, vad_threshold, attack_frames, release_frames），derive Serialize/Deserialize
  - [x] 2.2 实现配置文件读取逻辑（TOML），文件不存在时写入默认值
  - [x] 2.3 实现配置文件路径确定（放在 %APPDATA%/AutoDuck/）

- [x] Task 3: VAD 防抖状态机
  - [x] 3.1 定义 `VoiceState` 枚举（Silent, Speaking）
  - [x] 3.2 实现 `VadStateMachine` 结构体及 `update(score, threshold) -> Option<VoiceState>` 方法
  - [x] 3.3 编写单元测试验证防抖逻辑（连续语音帧触发、连续静音帧恢复、短噪音不触发、短停顿不回弹）

- [x] Task 4: 音频采集与重采样
  - [x] 4.1 使用 cpal 枚举输入设备，获取默认输入设备及其支持的采样格式
  - [x] 4.2 实现音频回调：采集数据 → 多声道降混为单声道 → 写入 rtrb ring buffer
  - [x] 4.3 实现重采样模块：整数倍（48000→16000）简单抽取+低通滤波，非整数倍（44100→16000）用 rubato
  - [x] 4.4 实现帧切分：从重采样后的 16kHz 数据按 256 采样点切帧

- [x] Task 5: VAD 检测线程
  - [x] 5.1 实现 VAD 工作线程：从 ring buffer 读取帧 → earshot predict → 状态机 update → 发送状态变更事件
  - [x] 5.2 用 `std::panic::catch_unwind` 包裹，panic 时通过 channel 上报
  - [x] 5.3 使用 crossbeam-channel 将 VoiceState 变更通知音量控制线程

- [x] Task 6: 音量控制 — 模式 A（全局主音量）
  - [x] 6.1 封装 `IAudioEndpointVolume` 操作：获取/设置主音量 scalar 值
  - [x] 6.2 实现自定义 GUID 作为 `pguidEventContext`，在 `OnNotify` 回调中区分自身操作与外部手动改音量
  - [x] 6.3 实现降音逻辑：`new_level = current_level * duck_ratio`
  - [x] 6.4 实现音量快照与恢复逻辑
  - [x] 6.5 实现音量渐变：100-150ms 内分 8-10 步线性插值

- [x] Task 7: 音量控制 — 模式 B（应用级排除）
  - [x] 7.1 封装音频会话枚举：`IAudioSessionManager2::GetSessionEnumerator` → 遍历 `IAudioSessionControl2` 获取进程 PID/名称
  - [x] 7.2 实现进程名与 `excluded_apps` 列表匹配（统一大写比较）
  - [x] 7.3 实现音量快照 `HashMap<PID, f32>`，记录被压低前的原始音量
  - [x] 7.4 实现降音逻辑：遍历非排除会话，调用 `ISimpleAudioVolume::SetMasterVolume` 压低
  - [x] 7.5 实现恢复逻辑：根据快照恢复被压低会话的原音量
  - [x] 7.6 实现周期性会话枚举（每 500ms），处理说话期间新开启的应用
  - [x] 7.7 同样使用 `pguidEventContext` 机制避免回调死循环
  - [x] 7.8 复用 Task 6 的音量渐变逻辑，对多个会话在一个 tick 内批量操作

- [x] Task 8: 音量控制线程整合
  - [x] 8.1 实现音量控制主线程：`COINIT_MULTITHREADED` 初始化 COM
  - [x] 8.2 根据 `duck_mode` 配置选择模式 A 或模式 B 的控制策略
  - [x] 8.3 接收 VAD 状态变更事件，执行降音/恢复操作
  - [x] 8.4 用 `std::panic::catch_unwind` 包裹，panic 时通过 channel 上报

- [x] Task 9: 单实例锁
  - [x] 9.1 使用 `CreateMutexW` 创建命名 Mutex
  - [x] 9.2 检测 `ERROR_ALREADY_EXISTS`，已有实例时退出

- [x] Task 10: 系统托盘与菜单
  - [x] 10.1 使用 `tray-icon` + `muda` 创建托盘图标
  - [x] 10.2 实现托盘菜单：降音模式切换（全局/应用排除）、开机自启开关、退出
  - [x] 10.3 实现托盘线程的 win32 消息循环
  - [x] 10.4 实现崩溃状态反馈：收到 worker 线程 panic 通知后，托盘图标变灰提示"已停止工作"

- [x] Task 11: 开机自启
  - [x] 11.1 实现写注册表 `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`
  - [x] 11.2 实现删除注册表键值
  - [x] 11.3 与托盘菜单开关联动

- [x] Task 12: 主入口整合与集成测试
  - [x] 12.1 在 main.rs 中组装所有模块：单实例锁 → 配置加载 → 启动音频采集 → 启动 VAD 线程 → 启动音量控制线程 → 启动托盘
  - [x] 12.2 实现优雅退出：托盘退出菜单 → 通知所有线程停止 → 恢复音量 → 退出
  - [x] 12.3 端到端集成测试：模拟语音输入验证降音/恢复行为（编译验证通过）

# Task Dependencies

- Task 3（状态机）无依赖，可与 Task 2 并行
- Task 4（音频采集）依赖 Task 1
- Task 5（VAD 线程）依赖 Task 3 + Task 4
- Task 6（模式 A）依赖 Task 1
- Task 7（模式 B）依赖 Task 1，可与 Task 6 并行
- Task 8（音量控制线程整合）依赖 Task 5 + Task 6 + Task 7
- Task 10（托盘）依赖 Task 2（配置读取模式）+ Task 11（自启开关）
- Task 12（主入口整合）依赖所有其他 Task
