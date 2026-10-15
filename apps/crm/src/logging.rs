//! Daily rolling logs in `logs/` (`crm.log.YYYY-MM-DD`, 14 files kept), plus
//! stderr in debug builds, and the panic hook.

use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Log files kept in `logs/`.
pub const MAX_LOG_FILES: usize = 14;

/// Starts logging into `dir`. Keep the returned guard alive until exit (it
/// flushes the file writer). Without a usable log folder, logs go to stderr.
pub fn init(dir: &Path) -> anyhow::Result<WorkerGuard> {
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("crm.log")
        .max_log_files(MAX_LOG_FILES)
        .build(dir)?;
    let (file, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file)
        .with_ansi(false);
    let stderr_layer = cfg!(debug_assertions)
        .then(|| tracing_subscriber::fmt::layer().with_writer(std::io::stderr));
    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer)
        .try_init()?;
    Ok(guard)
}

/// Logs every panic (with its location and thread). A panic on a thread that
/// contains it with `catch_unwind` (the release profile unwinds) is only
/// logged: the caller gets an error and the program keeps running. Any other
/// panic tells the user the program will close, then aborts so that promise
/// holds. No Slint window is opened from inside the hook.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let thread = std::thread::current();
        let name = thread.name();
        let contained = is_contained_thread(name);
        tracing::error!(
            "panic in thread {} at {location}: {payload}{}",
            name.unwrap_or("<unnamed>"),
            if contained { " (contained)" } else { "" }
        );
        default(info);
        if contained {
            return;
        }
        // The non-blocking log writer drains while the message box is open, so
        // the line above reaches the log file before the abort.
        crate::startup::native_message(crm_ui::shell::texts::UNEXPECTED);
        std::process::abort();
    }));
}

/// Threads that run their work inside `catch_unwind` and turn a panic into an
/// error for the caller. The names must match the spawn sites:
/// `crates/storage/src/db.rs` (`crm-db-writer`) and
/// `crates/ui/src/shell/worker.rs` (`crm-job-{id}`).
fn is_contained_thread(name: Option<&str>) -> bool {
    match name {
        Some("crm-db-writer") => true,
        Some(name) => name
            .strip_prefix("crm-job-")
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_contained_thread;

    #[test]
    fn writer_and_job_threads_are_contained() {
        assert!(is_contained_thread(Some("crm-db-writer")));
        assert!(is_contained_thread(Some("crm-job-1")));
        assert!(is_contained_thread(Some("crm-job-4096")));
    }

    #[test]
    fn other_threads_are_not_contained() {
        for name in [
            "main",
            "crm-activation",
            "crm-job",
            "crm-job-",
            "crm-job-x",
            "crm-db-writer-2",
            "crm-db",
            "tracing-appender",
            "",
        ] {
            assert!(!is_contained_thread(Some(name)), "{name}");
        }
        assert!(!is_contained_thread(None));
    }

    /// The spawn sites still use the names the hook knows.
    #[test]
    fn spawn_sites_use_the_known_names() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let writer = std::fs::read_to_string(format!("{root}/crates/storage/src/db.rs")).unwrap();
        assert!(writer.contains(r#".name("crm-db-writer".into())"#));
        let worker =
            std::fs::read_to_string(format!("{root}/crates/ui/src/shell/worker.rs")).unwrap();
        assert!(worker.contains(r#".name(format!("crm-job-{id}"))"#));
    }
}
