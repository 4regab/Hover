//! Windows: %APPDATA% (the roaming known folder, as Environment.SpecialFolder.
//! ApplicationData finds it), DPAPI for note.key, HKCU\…\Run for launch at login.

use std::path::{Path, PathBuf};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ};
use windows::Win32::UI::Shell::{FOLDERID_RoamingAppData, SHGetKnownFolderPath, KNOWN_FOLDER_FLAG};

pub fn app_data() -> Option<PathBuf> {
    unsafe {
        let p: PWSTR = SHGetKnownFolderPath(&FOLDERID_RoamingAppData, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

/// Path.GetFullPath: GetFullPathNameW, which std::path::absolute calls.
pub fn full_path(p: &Path) -> PathBuf { std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()) }

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

/// ProtectedData with DataProtectionScope.CurrentUser: CryptProtectData with no
/// entropy and CRYPTPROTECT_UI_FORBIDDEN, as .NET calls it, so either build opens
/// the other's note.key.
#[derive(Default)]
pub struct SystemKeyGuard;

fn dpapi(data: &[u8], protect: bool) -> Result<Vec<u8>, String> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        let r = if protect {
            CryptProtectData(&input, PCWSTR::null(), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        } else {
            CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        r.map_err(|e| e.message())?;
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
        Ok(bytes)
    }
}

impl crate::crypto::KeyGuard for SystemKeyGuard {
    fn wrap(&self, key: &[u8]) -> Result<Vec<u8>, String> { dpapi(key, true) }
    fn unwrap(&self, stored: &[u8]) -> Result<Vec<u8>, String> { dpapi(stored, false) }
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Hover";

#[derive(Default)]
pub struct SystemAutostart;

impl super::Autostart for SystemAutostart {
    fn enabled(&self) -> bool {
        let (k, v) = (wide(RUN_KEY), wide(RUN_VALUE));
        let mut len = 0u32;
        unsafe {
            RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(v.as_ptr()), RRF_RT_REG_SZ, None, None, Some(&mut len)).is_ok()
                // A REG_SZ of one terminating NUL is the empty string.
                && len > 2
        }
    }

    fn set(&self, on: bool) -> Result<(), String> {
        let (k, v) = (wide(RUN_KEY), wide(RUN_VALUE));
        unsafe {
            if on {
                let exe = std::env::current_exe().map_err(|e| e.to_string())?;
                let data = wide(&format!("\"{}\"", exe.display()));
                RegSetKeyValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(v.as_ptr()), REG_SZ.0, Some(data.as_ptr() as *const _), (data.len() * 2) as u32)
                    .ok().map_err(|e| e.message())
            } else {
                // throwOnMissingValue: false.
                let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(v.as_ptr()));
                Ok(())
            }
        }
    }
}
