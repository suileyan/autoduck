use std::panic::{AssertUnwindSafe, catch_unwind};

use anyhow::Result;
use crossbeam_channel::RecvTimeoutError;

use crate::config::AppConfig;
use crate::volume_control::VolumeController;

/// Commands sent to the volume worker thread.
pub enum VolumeCommand {
    Duck,
    Restore { ack: Option<crossbeam_channel::Sender<()>> },
    Stop,
    UpdateConfig(AppConfig),
}

/// Volume control worker that runs on its own thread.
///
/// Receives commands via a channel and delegates to the appropriate
/// `VolumeController` variant. Periodically refreshes audio sessions
/// (relevant for Mode B / per-app ducking).
pub struct VolumeWorker {
    controller: VolumeController,
    receiver: crossbeam_channel::Receiver<VolumeCommand>,
    duck_ratio: f32,
}

impl VolumeWorker {
    pub fn new(
        controller: VolumeController,
        receiver: crossbeam_channel::Receiver<VolumeCommand>,
        duck_ratio: f32,
    ) -> Self {
        Self {
            controller,
            receiver,
            duck_ratio,
        }
    }

    /// Run the worker loop. Returns `Ok(())` on normal exit (Stop command),
    /// or `Err` if the worker panicked.
    pub fn run(mut self) -> Result<()> {
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.run_inner();
        }));

        match result {
            Ok(()) => Ok(()),
            Err(panic_payload) => {
                let msg = if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "Volume worker panicked with unknown payload".to_string()
                };
                Err(anyhow::anyhow!("Volume worker panic: {}", msg))
            }
        }
    }

    fn run_inner(&mut self) {
        loop {
            match self.receiver.recv_timeout(std::time::Duration::from_millis(2000)) {
                Ok(VolumeCommand::Duck) => {
                    self.controller.duck(self.duck_ratio);
                }
                Ok(VolumeCommand::Restore { ack }) => {
                    self.controller.restore();
                    if let Some(ack) = ack {
                        let _ = ack.send(());
                    }
                }
                Ok(VolumeCommand::Stop) => {
                    break;
                }
                Ok(VolumeCommand::UpdateConfig(config)) => {
                    // Restore volume before replacing controller to avoid losing snapshots
                    self.controller.restore();
                    match VolumeController::new(
                        config.duck_mode,
                        config.excluded_apps.clone(),
                        config.duck_duration_ms,
                        config.restore_duration_ms,
                    ) {
                        Ok(new_controller) => {
                            self.controller = new_controller;
                            self.duck_ratio = config.duck_ratio;
                        }
                        Err(e) => {
                            eprintln!("更新配置时创建 VolumeController 失败: {}", e);
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    // Periodic session refresh (meaningful for Mode B, no-op for Mode A)
                    self.controller.refresh_sessions();
                }
                Err(RecvTimeoutError::Disconnected) => {
                    // Channel disconnected, exit gracefully
                    break;
                }
            }
        }
    }
}
