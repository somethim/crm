//! Native window helpers. On Windows they find the top-level window by its title
//! (`FindWindowW`; the single-instance lock makes the "CRM" main window unique)
//! because `raw-window-handle` is not a direct dependency of this crate. On other
//! platforms they do nothing.

#[cfg(test)]
thread_local! {
    /// The `set_window_enabled` calls (unit tests: Win32 is not visible headless).
    static ENABLED_CALLS: std::cell::RefCell<Vec<(String, bool)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The `set_window_enabled` calls since the last call (unit tests).
#[cfg(test)]
pub fn take_enabled_calls() -> Vec<(String, bool)> {
    ENABLED_CALLS.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

/// Enables or disables the native window titled `title` (true modality).
pub fn set_window_enabled(title: &str, enabled: bool) {
    #[cfg(test)]
    ENABLED_CALLS.with(|c| c.borrow_mut().push((title.to_owned(), enabled)));
    #[cfg(windows)]
    native::set_window_enabled(title, enabled);
    #[cfg(not(windows))]
    let _ = (title, enabled);
}

/// Restores the native window titled `title` and brings it to the foreground.
pub fn bring_to_front(title: &str) {
    #[cfg(windows)]
    native::bring_to_front(title);
    #[cfg(not(windows))]
    let _ = title;
}

/// The native handle of the window titled `title` (the owner of native
/// dialogs such as the print dialog).
#[cfg(windows)]
pub fn find_window(title: &str) -> Option<windows::Win32::Foundation::HWND> {
    native::find(title)
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod native {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };
    use windows::core::{HSTRING, PCWSTR};

    pub(super) fn find(title: &str) -> Option<HWND> {
        let title = HSTRING::from(title);
        // SAFETY: both arguments are valid for the call: a null class name and a
        // NUL-terminated wide string that outlives it.
        unsafe { FindWindowW(PCWSTR::null(), &title) }.ok()
    }

    pub(super) fn set_window_enabled(title: &str, enabled: bool) {
        if let Some(hwnd) = find(title) {
            // SAFETY: `hwnd` came from FindWindowW; EnableWindow tolerates a window
            // that was destroyed in the meantime (it fails).
            let _ = unsafe { EnableWindow(hwnd, enabled) };
        }
    }

    pub(super) fn bring_to_front(title: &str) {
        if let Some(hwnd) = find(title) {
            // SAFETY: `hwnd` came from FindWindowW; these calls only fail on a
            // stale handle. The second process called AllowSetForegroundWindow.
            unsafe {
                if IsIconic(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }
}
