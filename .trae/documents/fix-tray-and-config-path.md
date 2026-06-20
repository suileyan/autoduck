# 修复计划：托盘设置/退出 + 配置路径

## 问题分析

### Bug 1: 托盘点开一次设置后就点不开了
**根因**: `main.rs:165` 用 `gui_handle.is_finished()` 判断 GUI 线程是否仍在运行。但 GUI 线程调用 `slint::run_event_loop()` 后，即使窗口关闭，事件循环默认不会自动退出，线程永远不结束，`is_finished()` 永远返回 false。

**修复**: 在创建 GUI 窗口前调用 `slint::set_up_event_loop_quit_on_last_window_closed(true)`，使最后一个窗口关闭时事件循环自动退出。同时清理 `gui_handle`（join 已结束的线程）。

### Bug 2: 托盘无法正确退出
**根因**: 托盘线程 `run_tray()` 运行独立的 win32 消息循环，主循环收到 `TrayEvent::Quit` 后 break，但托盘线程没有退出机制——`PeekMessageW` 循环不会收到 `WM_QUIT`，`tray_handle.join()` 永远阻塞。

**修复**: 给 `run_tray` 添加一个 `running: Arc<AtomicBool>` 参数，循环条件检查此标志。主循环退出前设置 `running = false`，托盘线程检测到后退出循环。同时移除 `tray_handle.join()` 的无限等待，改为带超时的 join。

### Bug 3: 配置跟着应用目录走
**根因**: `config.rs:77-79` 使用 `%APPDATA%/AutoDuck/` 作为配置目录。用户希望配置文件放在 exe 同目录。

**修复**: 将 `config_dir()` 改为返回 exe 所在目录。使用 `std::env::current_exe()` 获取 exe 路径，取其父目录。

## 修改文件

### 1. `src/main.rs`
- 添加 `running_tray: Arc<AtomicBool>` 传给 `run_tray`
- GUI 线程创建前添加 `slint::set_up_event_loop_quit_on_last_window_closed(true)`
- GUI 线程结束后清理 `gui_handle`（join 并设为 None）
- 退出时设置 `running_tray.store(false)` 通知托盘线程退出
- `tray_handle.join()` 改为带超时（5秒）

### 2. `src/tray_icon.rs`
- `run_tray` 函数签名添加 `running: Arc<AtomicBool>` 参数
- 循环条件改为 `while running.load(Ordering::Relaxed)`
- 移除 `WM_QUIT` 检查（不再需要）

### 3. `src/config.rs`
- `config_dir()` 改为返回 exe 所在目录
- `config_file_path()` 返回 `exe所在目录/config.toml`

## 验证步骤
1. `cargo check` 编译通过
2. 代码审查：确认 GUI 窗口关闭后线程能退出
3. 代码审查：确认托盘退出时线程能正常结束
4. 代码审查：确认配置文件路径在 exe 同目录
