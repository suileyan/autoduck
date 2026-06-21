use anyhow::{Context, Result};
use std::collections::HashMap;
use std::thread;
use std::time::Duration;

use crate::config::DuckMode;
use windows::core::GUID;
use windows::core::Interface;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioSessionControl, IAudioSessionControl2,
    IAudioSessionManager2, ISimpleAudioVolume, IMMDevice,
    IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED, CLSCTX_ALL};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_NAME_FORMAT,
};
use windows::core::PWSTR;

/// Unique GUID used as pguidEventContext to identify our own volume changes.
static OUR_EVENT_CONTEXT: GUID = GUID::from_values(
    0xA1B2C3D4,
    0xE5F6,
    0x7890,
    [0xAB, 0xCD, 0xEF, 0x12, 0x34, 0x56, 0x78, 0x9A],
);

/// RAII guard for COM initialization. Calls CoUninitialize on drop.
///
/// # MTA Reference Counting
///
/// `CoInitializeEx(COINIT_MULTITHREADED)` initializes the COM Multi-Threaded Apartment (MTA),
/// which is reference-counted per thread. Each successful `CoInitializeEx` must be paired with
/// a `CoUninitialize`. When a `VolumeController` is replaced via `UpdateConfig`, the old
/// controller's guard drops and calls `CoUninitialize`, decrementing the MTA reference count.
/// This is safe because the MTA remains active as long as at least one reference exists
/// (the new controller holds one). The MTA is only fully cleaned up when the last reference
/// is released.
struct CoInitializeGuard;

impl CoInitializeGuard {
    /// Initialize COM and return a guard that will uninitialize on drop.
    fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .context("CoInitializeEx failed")?;
        }
        Ok(Self)
    }
}

impl Drop for CoInitializeGuard {
    fn drop(&mut self) {
        // Decrement MTA reference count; MTA remains active while other guards exist.
        unsafe {
            CoUninitialize();
        }
    }
}

// ---------------------------------------------------------------------------
// VolumeController enum — dispatches to Global or Apps mode
// ---------------------------------------------------------------------------

pub enum VolumeController {
    Global(GlobalVolumeController),
    Apps(AppsVolumeController),
}

impl VolumeController {
    pub fn new(mode: DuckMode, excluded_apps: Vec<String>, duck_duration_ms: u32, restore_duration_ms: u32) -> Result<Self> {
        match mode {
            DuckMode::Global => {
                let ctrl = GlobalVolumeController::new(duck_duration_ms, restore_duration_ms)?;
                Ok(VolumeController::Global(ctrl))
            }
            DuckMode::Apps => {
                let ctrl = AppsVolumeController::new(excluded_apps, duck_duration_ms, restore_duration_ms)?;
                Ok(VolumeController::Apps(ctrl))
            }
        }
    }

    pub fn duck(&mut self, ratio: f32) {
        match self {
            VolumeController::Global(ctrl) => ctrl.duck(ratio),
            VolumeController::Apps(ctrl) => ctrl.duck(ratio),
        }
    }

    pub fn restore(&mut self) {
        match self {
            VolumeController::Global(ctrl) => ctrl.restore(),
            VolumeController::Apps(ctrl) => ctrl.restore(),
        }
    }

    pub fn refresh_sessions(&mut self) {
        match self {
            VolumeController::Global(_) => { /* no-op for global mode */ }
            VolumeController::Apps(ctrl) => ctrl.refresh_sessions(),
        }
    }
}

// ---------------------------------------------------------------------------
// Mode A — Global master volume via IAudioEndpointVolume
// ---------------------------------------------------------------------------

pub struct GlobalVolumeController {
    _com_guard: CoInitializeGuard,
    endpoint_volume: IAudioEndpointVolume,
    volume_snapshot: Option<f32>,
    duck_duration_ms: u32,
    restore_duration_ms: u32,
}

// SAFETY: COM interfaces are safe to send between threads when used from
// a multi-threaded COM apartment (COINIT_MULTITHREADED), which we initialize.
unsafe impl Send for GlobalVolumeController {}

impl GlobalVolumeController {
    pub fn new(duck_duration_ms: u32, restore_duration_ms: u32) -> Result<Self> {
        let _com_guard = CoInitializeGuard::new()?;

        let endpoint_volume = unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(
                &MMDeviceEnumerator,
                None,
                CLSCTX_ALL,
            )
            .context("Failed to create IMMDeviceEnumerator")?;

            let device: IMMDevice = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .context("Failed to get default render device")?;

            device
                .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
                .context("Failed to activate IAudioEndpointVolume")?
        };

        Ok(Self {
            _com_guard,
            endpoint_volume,
            volume_snapshot: None,
            duck_duration_ms,
            restore_duration_ms,
        })
    }

    pub fn duck(&mut self, ratio: f32) {
        if self.volume_snapshot.is_some() {
            // Already ducked — skip to prevent further lowering
            return;
        }
        let current = self.get_current_volume();
        self.volume_snapshot = Some(current);
        let target = current * ratio;
        let steps = 10u32;
        let step_delay_ms = self.duck_duration_ms / steps;
        self.set_volume_gradual(target, steps, step_delay_ms as u64);
    }

    pub fn restore(&mut self) {
        if let Some(snapshot) = self.volume_snapshot.take() {
            let steps = 10u32;
            let step_delay_ms = self.restore_duration_ms / steps;
            self.set_volume_gradual(snapshot, steps, step_delay_ms as u64);
        }
    }

    pub fn get_current_volume(&self) -> f32 {
        unsafe {
            self.endpoint_volume
                .GetMasterVolumeLevelScalar()
                .unwrap_or(1.0)
        }
    }

    fn set_volume_gradual(&self, target: f32, steps: u32, step_delay_ms: u64) {
        let current = self.get_current_volume();
        let step_delta = (target - current) / steps as f32;
        for i in 1..=steps {
            let volume = current + step_delta * i as f32;
            let clamped = volume.clamp(0.0, 1.0);
            unsafe {
                let _ = self
                    .endpoint_volume
                    .SetMasterVolumeLevelScalar(clamped, &OUR_EVENT_CONTEXT as *const _);
            }
            if i < steps {
                thread::sleep(Duration::from_millis(step_delay_ms));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Mode B — Per-app session volume via IAudioSessionManager2
// ---------------------------------------------------------------------------

pub struct AppsVolumeController {
    _com_guard: CoInitializeGuard,
    session_manager: IAudioSessionManager2,
    excluded_apps: Vec<String>,
    volume_snapshots: HashMap<u32, f32>,
    duck_ratio: f32,
    duck_duration_ms: u32,
    restore_duration_ms: u32,
}

// SAFETY: COM interfaces are safe to send between threads when used from
// a multi-threaded COM apartment (COINIT_MULTITHREADED), which we initialize.
unsafe impl Send for AppsVolumeController {}

impl AppsVolumeController {
    pub fn new(excluded_apps: Vec<String>, duck_duration_ms: u32, restore_duration_ms: u32) -> Result<Self> {
        let _com_guard = CoInitializeGuard::new()?;

        let session_manager = unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(
                &MMDeviceEnumerator,
                None,
                CLSCTX_ALL,
            )
            .context("Failed to create IMMDeviceEnumerator")?;

            let device: IMMDevice = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .context("Failed to get default render device")?;

            device
                .Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)
                .context("Failed to activate IAudioSessionManager2")?
        };

        Ok(Self {
            _com_guard,
            session_manager,
            // Store excluded app names as-is; comparison uses eq_ignore_ascii_case
            excluded_apps,
            volume_snapshots: HashMap::new(),
            duck_ratio: 0.3,
            duck_duration_ms,
            restore_duration_ms,
        })
    }

    pub fn duck(&mut self, ratio: f32) {
        self.duck_ratio = ratio;
        let steps = 10u32;
        let step_delay_ms = self.duck_duration_ms / steps;
        let sessions = self.enumerate_sessions();
        for session in sessions {
            if let Some(simple_vol) = self.get_session_volume(&session) {
                if let Some(pid) = self.get_session_pid(&session) {
                    if pid == 0 {
                        continue;
                    }
                    let process_name = get_process_name(pid);
                    let is_excluded = process_name
                        .as_ref()
                        .map(|name| self.excluded_apps.iter().any(|ex| ex.eq_ignore_ascii_case(name)))
                        .unwrap_or(false);

                    if !is_excluded {
                        // Skip if already ducked (snapshot exists)
                        if self.volume_snapshots.contains_key(&pid) {
                            continue;
                        }
                        let current =
                            unsafe { simple_vol.GetMasterVolume().unwrap_or(1.0) };
                        self.volume_snapshots.insert(pid, current);
                        let target = current * ratio;
                        set_volume_gradual_session(&simple_vol, target, steps, step_delay_ms as u64);
                    }
                }
            }
        }
    }

    pub fn restore(&mut self) {
        let steps = 10u32;
        let step_delay_ms = self.restore_duration_ms / steps;
        let sessions = self.enumerate_sessions();
        for session in sessions {
            if let Some(simple_vol) = self.get_session_volume(&session) {
                if let Some(pid) = self.get_session_pid(&session) {
                    if let Some(&original) = self.volume_snapshots.get(&pid) {
                        set_volume_gradual_session(&simple_vol, original, steps, step_delay_ms as u64);
                    }
                }
            }
        }
        self.volume_snapshots.clear();
    }

    pub fn refresh_sessions(&mut self) {
        // Re-enumerate sessions. If currently ducked, duck any new non-excluded sessions.
        // This is called periodically (every 2s) from the worker thread.
        if self.volume_snapshots.is_empty() {
            return;
        }

        // We are currently in ducked state — duck any new sessions that appeared
        let sessions = self.enumerate_sessions();
        for session in sessions {
            if let Some(simple_vol) = self.get_session_volume(&session) {
                if let Some(pid) = self.get_session_pid(&session) {
                    if pid == 0 {
                        continue;
                    }
                    // Skip sessions we've already handled
                    if self.volume_snapshots.contains_key(&pid) {
                        continue;
                    }
                    let process_name = get_process_name(pid);
                    let is_excluded = process_name
                        .as_ref()
                        .map(|name| self.excluded_apps.iter().any(|ex| ex.eq_ignore_ascii_case(name)))
                        .unwrap_or(false);

                    if !is_excluded {
                        let current =
                            unsafe { simple_vol.GetMasterVolume().unwrap_or(1.0) };
                        self.volume_snapshots.insert(pid, current);
                        let target = current * self.duck_ratio;
                        let steps = 10u32;
                        let step_delay_ms = self.duck_duration_ms / steps;
                        set_volume_gradual_session(&simple_vol, target, steps, step_delay_ms as u64);
                    }
                }
            }
        }
    }

    fn enumerate_sessions(&self) -> Vec<IAudioSessionControl2> {
        let mut result = Vec::new();
        unsafe {
            let enumerator = match self.session_manager.GetSessionEnumerator() {
                Ok(e) => e,
                Err(_) => return result,
            };

            let count = match enumerator.GetCount() {
                Ok(c) => c,
                Err(_) => return result,
            };

            for i in 0..count {
                if let Ok(control) = enumerator.GetSession(i) {
                    if let Ok(control2) = control.cast::<IAudioSessionControl2>() {
                        result.push(control2);
                    }
                }
            }
        }
        result
    }

    fn get_session_volume(&self, session: &IAudioSessionControl2) -> Option<ISimpleAudioVolume> {
        // IAudioSessionControl2 inherits from IAudioSessionControl.
        // Cast to base interface first, then query ISimpleAudioVolume.
        let control: IAudioSessionControl = session.cast().ok()?;
        control.cast::<ISimpleAudioVolume>().ok()
    }

    fn get_session_pid(&self, session: &IAudioSessionControl2) -> Option<u32> {
        unsafe {
            let pid = session.GetProcessId().ok()?;
            Some(pid)
        }
    }
}

/// Gradually change volume for a single audio session.
fn set_volume_gradual_session(
    volume: &ISimpleAudioVolume,
    target: f32,
    steps: u32,
    step_delay_ms: u64,
) {
    let current = unsafe { volume.GetMasterVolume().unwrap_or(1.0) };
    let step_delta = (target - current) / steps as f32;
    for i in 1..=steps {
        let vol = current + step_delta * i as f32;
        let clamped = vol.clamp(0.0, 1.0);
        unsafe {
            let _ = volume.SetMasterVolume(clamped, &OUR_EVENT_CONTEXT as *const _);
        }
        if i < steps {
            thread::sleep(Duration::from_millis(step_delay_ms));
        }
    }
}

/// Enumerate current audio session process names.
/// This is a standalone function that initializes its own COM.
pub fn enumerate_audio_session_names() -> Vec<String> {
    let mut result = Vec::new();

    unsafe {
        // Initialize COM for this call
        let _com_guard = match CoInitializeGuard::new() {
            Ok(g) => g,
            Err(_) => return result,
        };

        let enumerator: IMMDeviceEnumerator = match CoCreateInstance(
            &MMDeviceEnumerator,
            None,
            CLSCTX_ALL,
        ) {
            Ok(e) => e,
            Err(_) => return result,
        };

        let device: IMMDevice = match enumerator.GetDefaultAudioEndpoint(eRender, eConsole) {
            Ok(d) => d,
            Err(_) => return result,
        };

        let session_manager: IAudioSessionManager2 = match device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) {
            Ok(s) => s,
            Err(_) => return result,
        };

        let session_enumerator = match session_manager.GetSessionEnumerator() {
            Ok(e) => e,
            Err(_) => return result,
        };

        let count = match session_enumerator.GetCount() {
            Ok(c) => c,
            Err(_) => return result,
        };

        for i in 0..count {
            if let Ok(control) = session_enumerator.GetSession(i) {
                if let Ok(control2) = control.cast::<IAudioSessionControl2>() {
                    if let Ok(pid) = control2.GetProcessId() {
                        if pid != 0 {
                            if let Some(name) = get_process_name(pid) {
                                if !result.iter().any(|r| r.eq_ignore_ascii_case(&name)) {
                                    result.push(name);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    result
}

/// Get the process executable name from a PID.
fn get_process_name(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let _guard = scopeguard::guard(handle, |h| {
            let _ = CloseHandle(h);
        });
        let mut buffer = [0u16; 260];
        let mut size = buffer.len() as u32;
        QueryFullProcessImageNameW(handle, PROCESS_NAME_FORMAT(0), PWSTR(buffer.as_mut_ptr()), &mut size).ok()?;
        let full_path = PWSTR(buffer.as_mut_ptr()).to_string().ok()?;
        full_path.rsplit('\\').next().map(|s| s.to_string())
    }
}
