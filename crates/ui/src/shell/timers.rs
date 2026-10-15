//! Periodic work: every 2 s `refresh_if_changed` (then the `data_changed`
//! subscribers and the status bar), every 30 s the idle-lock check, every
//! 15 min the daily backup check (F-018: a copy per calendar day also while
//! the program keeps running).

use crate::UiContext;
use crate::shell::{idle, status, worker};
use std::rc::Rc;
use std::time::Duration;

/// The data-change poll interval.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
/// The idle-lock check interval.
pub const IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(30);
/// The daily-backup check interval.
pub const BACKUP_CHECK_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// Starts both timers (kept alive by the context).
pub fn install(ctx: &Rc<UiContext>) {
    let refresh = slint::Timer::default();
    let weak = Rc::downgrade(ctx);
    refresh.start(slint::TimerMode::Repeated, REFRESH_INTERVAL, move || {
        if let Some(ctx) = weak.upgrade() {
            poll(&ctx);
        }
    });
    ctx.keep_timer(refresh);

    let idle_timer = slint::Timer::default();
    let weak = Rc::downgrade(ctx);
    idle_timer.start(slint::TimerMode::Repeated, IDLE_CHECK_INTERVAL, move || {
        if let Some(ctx) = weak.upgrade() {
            idle::check(&ctx);
        }
    });
    ctx.keep_timer(idle_timer);

    let backup_timer = slint::Timer::default();
    let weak = Rc::downgrade(ctx);
    backup_timer.start(
        slint::TimerMode::Repeated,
        BACKUP_CHECK_INTERVAL,
        move || {
            if let Some(ctx) = weak.upgrade() {
                backup_check(&ctx, || {});
            }
        },
    );
    ctx.keep_timer(backup_timer);
}

/// One daily-backup check, off the UI thread and without the busy overlay
/// (it must not interrupt typing). A new copy updates the status bar here: the
/// poll does not report this facade's own commits (F-001). A failure is only
/// logged (the next check retries). `after` runs on the UI thread at the end.
pub fn backup_check(ctx: &Rc<UiContext>, after: impl FnOnce() + 'static) {
    let weak = Rc::downgrade(ctx);
    worker::run_quiet(
        ctx,
        |crm| crm.daily_backup_if_due(),
        move |result| {
            match result {
                Ok(true) => {
                    tracing::info!("daily backup taken");
                    if let Some(ctx) = weak.upgrade() {
                        status::update(&ctx);
                    }
                }
                Ok(false) => {}
                Err(e) => tracing::warn!("daily backup check failed: {e}"),
            }
            after();
        },
    );
}

/// One data-change poll.
pub fn poll(ctx: &UiContext) {
    if ctx.crm.refresh_if_changed() {
        ctx.notify_data_changed();
        status::update(ctx);
    }
}
