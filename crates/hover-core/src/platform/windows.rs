//! Windows: %APPDATA% (the roaming known folder, as Environment.SpecialFolder.
//! ApplicationData finds it), DPAPI for note.key, HKCU\…\Run for launch at login.

use std::path::{Path, PathBuf};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ};
use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, FOLDERID_Profile, FOLDERID_ProgramFiles, FOLDERID_RoamingAppData, SHGetKnownFolderPath, KNOWN_FOLDER_FLAG};

fn known(id: &windows::core::GUID) -> Option<PathBuf> {
    unsafe {
        let p: PWSTR = SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

/// Environment.SpecialFolder.ApplicationData.
pub fn app_data() -> Option<PathBuf> { known(&FOLDERID_RoamingAppData) }
/// UserProfile, LocalApplicationData and ProgramFiles, as Palette.Installed asks for them.
pub fn home() -> Option<PathBuf> { known(&FOLDERID_Profile) }
pub fn local_app_data() -> Option<PathBuf> { known(&FOLDERID_LocalAppData) }
pub fn program_files() -> Option<PathBuf> { known(&FOLDERID_ProgramFiles) }
/// Only Linux keeps other apps' settings in one config folder.
pub fn config_dir() -> Option<PathBuf> { None }

// MARK: The look: Theme.SystemDark and Animator.Still

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look { pub dark: bool, pub animations: bool }

/// Windows keeps "app mode" per user; a missing value means the light default. Motion
/// follows "Animation effects" (SystemParameters.ClientAreaAnimation).
pub fn look() -> Look {
    use windows::Win32::System::Registry::RRF_RT_REG_DWORD;
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    let (k, v) = (wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"), wide("AppsUseLightTheme"));
    let mut light = 1u32;
    let mut len = 4u32;
    let read = unsafe { RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(v.as_ptr()), RRF_RT_REG_DWORD, None, Some(&mut light as *mut u32 as *mut _), Some(&mut len)) };
    let dark = read.is_ok() && light == 0;
    let mut on = windows::core::BOOL(1);
    let got = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut on as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
    Look { dark, animations: got.is_err() || on.as_bool() }
}

/// UserPreferenceChanged's stand-in: the look read again every second, and `changed`
/// called when it differs (see the report: a registry read, no hidden window).
pub fn watch_look(changed: impl Fn() + Send + 'static) {
    std::thread::Builder::new().name("look".into()).spawn(move || {
        let mut last = look();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let now = look();
            if now != last { last = now; changed(); }
        }
    }).expect("a thread to watch the look");
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
    /// DPAPI refusing is for good (another user's blob, a reset password or profile).
    fn unwrap(&self, stored: &[u8]) -> Result<Vec<u8>, crate::crypto::KeyError> { dpapi(stored, false).map_err(crate::crypto::KeyError::never) }
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
