use anyhow::{bail, Result};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;

const MUTEX_NAME: &str = "AutoDuckSingleInstanceMutex";

pub struct SingleInstance {
    handle: HANDLE,
}

impl SingleInstance {
    pub fn new() -> Result<Self> {
        let mutex_name = HSTRING::from(MUTEX_NAME);

        let handle = unsafe { CreateMutexW(None, false, PCWSTR(mutex_name.as_ptr()))? };

        let last_error = unsafe { GetLastError() };
        if last_error == ERROR_ALREADY_EXISTS {
            unsafe { let _ = CloseHandle(handle); }
            bail!("另一个 AutoDuck 实例正在运行");
        }

        Ok(Self { handle })
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}
