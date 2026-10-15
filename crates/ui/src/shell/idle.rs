//! Idle time for the automatic lock. Windows: `GetLastInputInfo` against
//! `GetTickCount64` (system-wide input). Other platforms: unsupported (`None`),
//! so the idle lock is disabled there.

use std::time::Duration;

/// How long the user has been idle, or `None` where it cannot be measured.
pub fn idle_time() -> Option<Duration> {
    #[cfg(windows)]
    {
        native::idle_time()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod native {
    use std::time::Duration;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    // `GetTickCount64` lives in `Win32_System_SystemInformation`, which is not an
    // enabled feature of the `windows` crate here; declared from kernel32 directly.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetTickCount64() -> u64;
    }

    pub(super) fn idle_time() -> Option<Duration> {
        let mut info = LASTINPUTINFO {
            cbSize: size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        // SAFETY: `info` is a valid, initialised LASTINPUTINFO with cbSize set;
        // the call only writes into it.
        let ok = unsafe { GetLastInputInfo(&mut info) }.as_bool();
        if !ok {
            return None;
        }
        // SAFETY: GetTickCount64 has no preconditions.
        let now = unsafe { GetTickCount64() };
        // dwTime is the low 32 bits of the tick count (it wraps every 49.7 days).
        let idle_ms = (now as u32).wrapping_sub(info.dwTime);
        Some(Duration::from_millis(u64::from(idle_ms)))
    }
}

/// The 30 s check: locks when the idle time reached the configured minutes.
pub fn check(ctx: &crate::UiContext) {
    let minutes = ctx.idle_minutes.get();
    if minutes == 0 || ctx.is_locked() {
        return;
    }
    if let Some(idle) = idle_time()
        && idle >= Duration::from_secs(u64::from(minutes) * 60)
    {
        tracing::info!("idle for {} s: locking", idle.as_secs());
        ctx.lock();
    }
}

/// Reads the idle-lock minutes into the context (start and after Settings).
pub fn load_setting(ctx: &crate::UiContext) {
    let minutes = ctx.crm.idle_lock_minutes().unwrap_or_else(|e| {
        tracing::warn!("idle lock setting unreadable: {e}");
        0
    });
    ctx.idle_minutes.set(minutes);
    if minutes > 0 && idle_time().is_none() {
        tracing::info!("idle lock is set to {minutes} min but is not supported on this platform");
    }
}
