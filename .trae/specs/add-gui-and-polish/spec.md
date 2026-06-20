# AutoDuck GUI 与体验优化 Spec

## Why

v1.0.0-beta 仅有托盘菜单和 TOML 文件配置，用户无法直观地调整参数或选择排除应用。同时 release 构建启动时会弹出终端窗口，且 GUI 未适配高分辨率显示器。需要添加图形配置界面、消除终端窗口、并确保高 DPI 兼容。

## What Changes

- 添加 Windows 子系统设置为 `windows`，消除 release 构建的终端窗口
- 添加高 DPI 感知声明（DPI manifest），确保 GUI 在高分辨率设备上不模糊
- 添加 GUI 配置窗口（基于 `slint`），支持所有配置项的可视化编辑
- 添加应用排除模式的应用选择功能：枚举当前音频会话进程，用户可勾选排除
- 扩展配置项：降音渐变时长、恢复渐变时长、VAD 灵敏度等
- 每步完成后安全审查、提交推送

## Impact

- Affected specs: 配置系统、托盘 UI、音量控制
- Affected code: `Cargo.toml`、`main.rs`、`config.rs`、`tray_icon.rs`、新增 GUI 模块

---

## ADDED Requirements

### Requirement: 无终端窗口启动

系统 SHALL 在 release 构建中不显示控制台窗口。通过在 `main.rs` 中设置 `#![windows_subsystem = "windows"]` 实现。

#### Scenario: Release 构建启动
- **WHEN** 用户双击运行 release 版 autoduck.exe
- **THEN** 不弹出任何终端/控制台窗口，仅显示托盘图标

#### Scenario: Debug 构建启动
- **WHEN** 开发者运行 debug 版本
- **THEN** 保留控制台窗口以便查看日志输出

### Requirement: 高 DPI 显示支持

系统 SHALL 在高分辨率显示器上正确渲染 GUI 界面，不出现模糊或缩放异常。通过 Windows 应用 manifest 声明 DPI 感知实现。

#### Scenario: 150% 缩放显示器
- **WHEN** 用户在 150% DPI 缩放的显示器上打开 GUI
- **THEN** GUI 界面清晰渲染，文字和控件不模糊

#### Scenario: 多显示器不同 DPI
- **WHEN** 用户将 GUI 窗口拖动到不同 DPI 的显示器
- **THEN** GUI 自动适配目标显示器的 DPI

### Requirement: GUI 配置窗口

系统 SHALL 提供图形化配置窗口，用户可通过托盘菜单"设置"打开。GUI 基于 `slint` 框架实现。

#### Scenario: 打开设置窗口
- **WHEN** 用户在托盘菜单点击"设置"
- **THEN** 弹出配置窗口，显示所有可配置项

#### Scenario: 修改降音比例
- **WHEN** 用户在 GUI 中拖动降音比例滑块
- **THEN** 实时预览效果，点击"应用"后保存到配置文件

#### Scenario: 切换降音模式
- **WHEN** 用户在 GUI 中切换全局/应用排除模式
- **THEN** 模式立即切换，配置持久化

### Requirement: 扩展配置项

系统 SHALL 支持以下新增配置项，均可通过 GUI 调整：

| 配置项 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `duck_duration_ms` | u32 | 120 | 降音渐变时长（ms） |
| `restore_duration_ms` | u32 | 120 | 恢复渐变时长（ms） |
| `duck_ratio` | f32 | 0.3 | 降音比例（已有，GUI 可调） |
| `vad_threshold` | f32 | 0.5 | VAD 灵敏度（已有，GUI 可调） |
| `attack_frames` | u32 | 4 | 触发防抖帧数（已有，GUI 可调） |
| `release_frames` | u32 | 30 | 释放防抖帧数（已有，GUI 可调） |
| `duck_mode` | enum | global | 降音模式（已有，GUI 可切换） |
| `excluded_apps` | Vec<String> | [] | 排除应用列表（已有，GUI 可编辑） |

#### Scenario: 修改渐变时长
- **WHEN** 用户修改降音渐变时长为 200ms
- **THEN** 后续降音操作在 200ms 内完成渐变

### Requirement: 应用排除选择器

系统 SHALL 在 GUI 中提供应用排除选择器，枚举当前所有音频会话的进程名，用户可勾选要排除的应用。

#### Scenario: 查看当前音频应用
- **WHEN** 用户打开应用排除选择器
- **THEN** 显示当前所有正在播放音频的应用列表，已排除的应用被勾选

#### Scenario: 添加排除应用
- **WHEN** 用户勾选一个应用
- **THEN** 该应用加入排除列表，音量不再被压低，配置持久化

#### Scenario: 移除排除应用
- **WHEN** 用户取消勾选一个应用
- **THEN** 该应用从排除列表移除，音量可被压低，配置持久化

#### Scenario: 手动输入进程名
- **WHEN** 用户在输入框中手动输入进程名并添加
- **THEN** 该进程名加入排除列表（统一大写存储），配置持久化

### Requirement: 安全性

系统 SHALL 在以下方面确保安全：

- GUI 进程列表仅显示进程名，不暴露完整路径或 PID
- 手动输入进程名时进行白名单字符校验（仅允许字母、数字、下划线、点、连字符）
- 配置文件写入使用原子操作（先写临时文件再 rename），防止写入中断导致配置丢失
- GUI 窗口不响应外部消息注入（无自定义 Windows 消息处理）
- 托盘菜单事件与 GUI 事件通过内部 channel 传递，不接受外部输入

#### Scenario: 恶意进程名注入
- **WHEN** 用户尝试输入包含路径分隔符或特殊字符的进程名（如 `..\..\evil.exe`）
- **THEN** 输入被拒绝，提示"无效的进程名"

#### Scenario: 配置写入中断
- **WHEN** 配置文件写入过程中程序崩溃
- **THEN** 原配置文件保持完整（因使用原子写入）

## MODIFIED Requirements

### Requirement: 系统托盘后台运行

系统 SHALL 以托盘图标方式后台运行，无主窗口。托盘菜单提供：**设置**、模式切换、开机自启开关、退出。

（新增"设置"菜单项，打开 GUI 配置窗口）

## REMOVED Requirements

无。
