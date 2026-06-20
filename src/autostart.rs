use anyhow::Result;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::*;

const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "AutoDuck";

pub fn is_auto_start_enabled() -> bool {
    unsafe {
        let mut h_key = HKEY::default();
        let key_path = HSTRING::from(RUN_KEY_PATH);
        let result = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_path.as_ptr()),
            None,
            KEY_READ,
            &mut h_key,
        );

        if result != ERROR_SUCCESS {
            return false;
        }

        let value_name = HSTRING::from(VALUE_NAME);
        let result = RegQueryValueExW(
            h_key,
            PCWSTR(value_name.as_ptr()),
            None,
            None,
            None,
            None,
        );

        let _ = RegCloseKey(h_key);

        result == ERROR_SUCCESS
    }
}

pub fn enable_auto_start() -> Result<()> {
    unsafe {
        let mut h_key = HKEY::default();
        let key_path = HSTRING::from(RUN_KEY_PATH);
        let result = RegCreateKeyW(
            HKEY_CURRENT_USER,
            PCWSTR(key_path.as_ptr()),
            &mut h_key,
        );

        if result != ERROR_SUCCESS {
            anyhow::bail!("RegCreateKeyW failed with error code: {}", result.0);
        }

        let exe_path = std::env::current_exe()?;
        let exe_path_str = exe_path.to_string_lossy().to_string();
        let exe_path_wide = HSTRING::from(exe_path_str.as_str());

        // Build the wide string bytes including null terminator
        let mut value_bytes: Vec<u8> = Vec::new();
        for ch in exe_path_wide.iter() {
            let le_bytes = ch.to_le_bytes();
            value_bytes.extend_from_slice(&le_bytes);
        }
        // Null terminator (two zero bytes)
        value_bytes.push(0);
        value_bytes.push(0);

        let result = RegSetValueExW(
            h_key,
            PCWSTR(HSTRING::from(VALUE_NAME).as_ptr()),
            None,
            REG_SZ,
            Some(&value_bytes),
        );

        let _ = RegCloseKey(h_key);

        if result != ERROR_SUCCESS {
            anyhow::bail!("RegSetValueExW failed with error code: {}", result.0);
        }

        Ok(())
    }
}

pub fn disable_auto_start() -> Result<()> {
    unsafe {
        let mut h_key = HKEY::default();
        let key_path = HSTRING::from(RUN_KEY_PATH);
        let result = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_path.as_ptr()),
            None,
            KEY_SET_VALUE,
            &mut h_key,
        );

        if result != ERROR_SUCCESS {
            // Key doesn't exist or can't be opened, nothing to delete
            return Ok(());
        }

        let value_name = HSTRING::from(VALUE_NAME);
        let result = RegDeleteValueW(h_key, PCWSTR(value_name.as_ptr()));

        let _ = RegCloseKey(h_key);

        if result != ERROR_SUCCESS {
            // Value doesn't exist is not an error
            return Ok(());
        }

        Ok(())
    }
}
