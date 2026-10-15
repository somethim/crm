//! Single-instance focus. The instance lock (`data/crm.lock`, crm-storage)
//! decides who runs; this module lets a second launch wake the first one.
//!
//! Windows: a named auto-reset event `Local\CRM-activate-{fnv64(exe_dir)}`. The
//! running instance waits on it on a thread; a second launch opens it, allows
//! the first to take the foreground, and sets it. Elsewhere: no-op (the second
//! launch shows "Programi është tashmë i hapur.").

use crm_ui::ActivationListener;
use std::path::Path;

/// 64-bit FNV-1a.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The event name for the program folder (case-insensitive, like Windows paths).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn event_name(exe_dir: &Path) -> String {
    let key = exe_dir.to_string_lossy().to_lowercase();
    format!("Local\\CRM-activate-{:016x}", fnv64(key.as_bytes()))
}

/// Wakes the running instance. Returns false when there is none to wake (or
/// on platforms without the mechanism).
pub fn signal_existing(exe_dir: &Path) -> bool {
    #[cfg(windows)]
    {
        native::signal(&event_name(exe_dir))
    }
    #[cfg(not(windows))]
    {
        let _ = exe_dir;
        false
    }
}

/// Creates the activation event now (so an early second launch is not lost)
/// and returns the hook that starts waiting for it once the UI runs.
pub fn listener(exe_dir: &Path) -> Option<ActivationListener> {
    #[cfg(windows)]
    {
        native::listener(&event_name(exe_dir))
    }
    #[cfg(not(windows))]
    {
        let _ = exe_dir;
        None
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod native {
    use crm_ui::ActivationListener;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        CreateEventW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent, WaitForSingleObject,
    };
    use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
    use windows::core::HSTRING;

    pub(super) fn signal(name: &str) -> bool {
        let name = HSTRING::from(name);
        // SAFETY: `name` is a valid NUL-terminated wide string for the call.
        let Ok(event) = (unsafe { OpenEventW(EVENT_MODIFY_STATE, false, &name) }) else {
            return false;
        };
        // SAFETY: `event` is a valid handle opened above and closed exactly once;
        // AllowSetForegroundWindow has no pointer arguments.
        unsafe {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            let set = SetEvent(event).is_ok();
            let _ = CloseHandle(event);
            set
        }
    }

    pub(super) fn listener(name: &str) -> Option<ActivationListener> {
        let name = HSTRING::from(name);
        // `CreateEventW` needs the `Win32_Security` feature of `windows`, which
        // crm-storage enables (features are unified for this binary).
        // SAFETY: no security attributes; `name` is a valid wide string.
        let event = match unsafe { CreateEventW(None, false, false, &name) } {
            Ok(event) => event,
            Err(e) => {
                tracing::warn!("activation event not created: {e}");
                return None;
            }
        };
        // Raw handles are not `Send`; the value is moved to the waiting thread.
        let raw = event.0 as isize;
        Some(Box::new(move |activate: Box<dyn Fn() + Send + Sync>| {
            let spawned = std::thread::Builder::new()
                .name("crm-activation".into())
                .spawn(move || {
                    let event = HANDLE(raw as *mut core::ffi::c_void);
                    loop {
                        // SAFETY: `event` stays open for the process lifetime (it is
                        // never closed), so waiting on it is valid.
                        let result = unsafe { WaitForSingleObject(event, INFINITE) };
                        if result != WAIT_OBJECT_0 {
                            tracing::warn!("activation wait ended: {result:?}");
                            break;
                        }
                        activate();
                    }
                });
            if let Err(e) = spawned {
                tracing::warn!("activation thread not started: {e}");
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv64_matches_the_reference_vectors() {
        assert_eq!(fnv64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv64(b"foobar"), 0x85944171f73967e8);
    }

    #[test]
    fn event_name_is_stable_and_case_insensitive() {
        let name = event_name(Path::new(r"E:\CRM"));
        assert_eq!(name, "Local\\CRM-activate-b3485007436705be");
        assert_eq!(name, event_name(Path::new(r"e:\crm")));
        assert_ne!(name, event_name(Path::new(r"F:\CRM")));
        assert!(name.starts_with("Local\\CRM-activate-"));
        assert_eq!(name.len(), "Local\\CRM-activate-".len() + 16);
    }
}
