# Tasks

- [ ] Task 1: 消除终端窗口 + 高 DPI 支持
  - [ ] 1.1 在 main.rs 顶部添加条件编译 `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`
  - [ ] 1.2 添加 Windows DPI 感知 manifest：创建 `.windows/app.manifest` 声明 `dpiAware` + `dpiAwareness`，在 `.cargo/config.toml` 中配置链接
  - [ ] 1.3 安全审查：确认无新攻击面
  - [ ] 1.4 提交推送：`feat: hide console window and add DPI awareness`

- [ ] Task 2: 扩展配置项
  - [ ] 2.1 在 `AppConfig` 中添加 `duck_duration_ms: u32`（默认 120）和 `restore_duration_ms: u32`（默认 120）
  - [ ] 2.2 修改 `volume_control.rs` 的渐变逻辑，使用配置中的时长而非硬编码
  - [ ] 2.3 实现配置文件原子写入（先写临时文件再 rename）
  - [ ] 2.4 添加进程名白名单校验函数 `validate_process_name(name: &str) -> bool`
  - [ ] 2.5 安全审查：验证原子写入和输入校验
  - [ ] 2.6 提交推送：`feat: extend config with duck/restore duration and atomic save`

- [ ] Task 3: 添加 slint GUI 框架
  - [ ] 3.1 添加 `slint` 依赖到 Cargo.toml
  - [ ] 3.2 创建 `src/gui.slint` UI 定义文件（窗口布局、控件）
  - [ ] 3.3 创建 `src/gui.rs` 模块，封装 GUI 窗口的创建和事件处理
  - [ ] 3.4 安全审查：确认 slint 无自定义消息处理、无外部输入接受
  - [ ] 3.5 提交推送：`feat: add slint GUI framework and layout`

- [ ] Task 4: GUI 配置面板实现
  - [ ] 4.1 实现降音模式切换（全局/应用排除）单选按钮
  - [ ] 4.2 实现降音比例滑块（0.0 - 1.0）
  - [ ] 4.3 实现 VAD 灵敏度滑块（0.0 - 1.0）
  - [ ] 4.4 实现防抖参数调节（attack_frames / release_frames）
  - [ ] 4.5 实现渐变时长调节（duck_duration_ms / restore_duration_ms）
  - [ ] 4.6 实现"应用"和"重置"按钮，应用时保存配置并通知运行时
  - [ ] 4.7 安全审查：验证所有输入范围校验
  - [ ] 4.8 提交推送：`feat: implement GUI configuration panel`

- [ ] Task 5: 应用排除选择器
  - [ ] 5.1 实现枚举当前音频会话进程列表（复用 volume_control.rs 的会话枚举逻辑）
  - [ ] 5.2 在 GUI 中显示进程列表，支持勾选排除
  - [ ] 5.3 实现手动输入进程名并添加（带白名单校验）
  - [ ] 5.4 实现排除列表的增删操作，实时生效
  - [ ] 5.5 安全审查：验证进程名校验、无路径泄露
  - [ ] 5.6 提交推送：`feat: add app exclusion selector in GUI`

- [ ] Task 6: 托盘菜单集成 + 运行时配置热更新
  - [ ] 6.1 托盘菜单添加"设置"项，点击打开 GUI 窗口
  - [ ] 6.2 实现运行时配置热更新：GUI 修改配置后通过 channel 通知主循环
  - [ ] 6.3 主循环收到配置变更后通知音量控制线程更新参数
  - [ ] 6.4 安全审查：验证 channel 通信无外部可注入风险
  - [ ] 6.5 提交推送：`feat: integrate GUI with tray and runtime config reload`

- [ ] Task 7: 最终审查与 v1.1.0-beta 发布
  - [ ] 7.1 全量安全审查：检查所有新增代码的攻击面
  - [ ] 7.2 编译 release 版本并验证：无终端窗口、高 DPI 正常、GUI 功能完整
  - [ ] 7.3 更新 README 文档
  - [ ] 7.4 提交推送并打 tag v1.1.0-beta

# Task Dependencies

- Task 1 无依赖，可立即开始
- Task 2 无依赖，可与 Task 1 并行
- Task 3 依赖 Task 1（高 DPI manifest 需先就位）
- Task 4 依赖 Task 2 + Task 3
- Task 5 依赖 Task 3 + Task 4
- Task 6 依赖 Task 4 + Task 5
- Task 7 依赖所有其他 Task
