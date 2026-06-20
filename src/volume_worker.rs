use std::panic::{AssertUnwindSafe, catch_unwind};

use anyhow::Result;
use crossbeam_channel::RecvTimeoutError;

use crate::volume_control::VolumeController;

/// Commands sent to the volume worker thread.
pub enum VolumeCommand {
    Duck,
    Restore,
    Stop,
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
                Ok(VolumeCommand::Restore) => {
                    self.controller.restore();
                }
                Ok(VolumeCommand::Stop) => {
                    break;
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
