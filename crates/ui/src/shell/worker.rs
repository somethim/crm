//! Background work: run a services call on a worker thread, deliver the result
//! on the UI thread, with the busy overlay shown in between.

use crate::shell::{modal, texts};
use crate::{MessageDialog, UiContext};
use crm_services::Crm;
use slint::ComponentHandle;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::rc::{Rc, Weak};

type Completion = Box<dyn FnOnce(Option<Box<dyn Any + Send>>)>;

thread_local! {
    /// Completions waiting for their worker, by job id (UI thread only).
    static PENDING: RefCell<HashMap<u64, Completion>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
    /// The message shown for the last job that failed (for tests).
    static LAST_FAILURE: RefCell<Option<slint::Weak<MessageDialog>>> = const { RefCell::new(None) };
    /// Headless tests without an event loop run jobs inline (see [`set_inline_for_tests`]).
    static INLINE: Cell<bool> = const { Cell::new(false) };
}

/// Runs every job of this (UI) thread inline, before the call returns: for the
/// headless tests, which have no event loop to deliver results through.
#[doc(hidden)]
pub fn set_inline_for_tests(inline: bool) {
    INLINE.with(|i| i.set(inline));
}

fn inline() -> bool {
    INLINE.with(Cell::get)
}

/// Runs `job` on a new thread, then `done` with its result on the UI thread;
/// no `Crm`, no busy overlay (the start-up gate shows its own busy state). A
/// panicking job is logged and `done` never runs.
pub fn run_plain<T, J, D>(job: J, done: D)
where
    T: Send + 'static,
    J: FnOnce() -> T + Send + 'static,
    D: FnOnce(T) + 'static,
{
    if inline() {
        if let Ok(value) = std::panic::catch_unwind(AssertUnwindSafe(job)) {
            done(value);
        }
        return;
    }
    let id = NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    let completion: Completion = Box::new(move |result| {
        if let Some(Ok(value)) = result.map(|r| r.downcast::<T>()) {
            done(*value);
        }
    });
    PENDING.with(|p| p.borrow_mut().insert(id, completion));
    let spawned = std::thread::Builder::new()
        .name(format!("crm-job-{id}"))
        .spawn(move || {
            let result = std::panic::catch_unwind(AssertUnwindSafe(job))
                .ok()
                .map(|value| Box::new(value) as Box<dyn Any + Send>);
            if result.is_none() {
                tracing::error!("background job {id} panicked");
            }
            if let Err(e) = slint::invoke_from_event_loop(move || complete(id, result)) {
                tracing::warn!("background job {id}: event loop gone: {e}");
            }
        });
    if let Err(e) = spawned {
        tracing::error!("background job {id}: thread spawn failed: {e}");
        complete(id, None);
    }
}

/// The "unexpected error" message of the last failed job, while it is shown.
pub fn last_job_failure() -> Option<MessageDialog> {
    LAST_FAILURE
        .with(|d| d.borrow().as_ref().and_then(slint::Weak::upgrade))
        .filter(|d| d.window().is_visible())
}

/// Runs `job` with a clone of the [`Crm`] on a new thread, then `done` with its
/// result on the UI thread (through `slint::invoke_from_event_loop`). The busy
/// overlay ("Duke punuar…") covers the main window while any job runs, and the
/// list windows are disabled and refuse writes meanwhile (F-029). A job
/// that panics (or whose thread cannot start) is logged and its `done` never
/// runs: the overlay is removed and the user gets the standard error message
/// ("Ndodhi një gabim i papritur. Veprimi nuk u krye.", F-028).
pub fn run_bg<T, J, D>(ctx: &Rc<UiContext>, job: J, done: D)
where
    T: Send + 'static,
    J: FnOnce(Crm) -> T + Send + 'static,
    D: FnOnce(T) + 'static,
{
    run(ctx, true, job, done);
}

/// As [`run_bg`] without the busy overlay: for periodic housekeeping (the daily
/// backup check) that must not cover the window or take the keyboard focus.
/// A panic is only logged here: the user did not start the job, and a message
/// box would take the focus from whatever they are typing.
pub fn run_quiet<T, J, D>(ctx: &Rc<UiContext>, job: J, done: D)
where
    T: Send + 'static,
    J: FnOnce(Crm) -> T + Send + 'static,
    D: FnOnce(T) + 'static,
{
    run(ctx, false, job, done);
}

fn run<T, J, D>(ctx: &Rc<UiContext>, busy: bool, job: J, done: D)
where
    T: Send + 'static,
    J: FnOnce(Crm) -> T + Send + 'static,
    D: FnOnce(T) + 'static,
{
    let id = NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    let weak: Weak<UiContext> = Rc::downgrade(ctx);
    let completion: Completion = Box::new(move |result| {
        if busy && let Some(ctx) = weak.upgrade() {
            ctx.end_busy();
        }
        let Some(result) = result else {
            if busy && let Some(ctx) = weak.upgrade() {
                show_failure(&ctx);
            }
            return;
        };
        match result.downcast::<T>() {
            Ok(value) => done(*value),
            Err(_) => tracing::error!("background job {id}: unexpected result type"),
        }
    });
    PENDING.with(|p| p.borrow_mut().insert(id, completion));
    if busy {
        ctx.begin_busy();
    }

    let crm = ctx.crm.clone();
    if inline() {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| job(crm)))
            .ok()
            .map(|value| Box::new(value) as Box<dyn Any + Send>);
        complete(id, result);
        return;
    }
    let spawned = std::thread::Builder::new()
        .name(format!("crm-job-{id}"))
        .spawn(move || {
            let result = match std::panic::catch_unwind(AssertUnwindSafe(|| job(crm))) {
                Ok(value) => Some(Box::new(value) as Box<dyn Any + Send>),
                Err(_) => {
                    tracing::error!("background job {id} panicked");
                    None
                }
            };
            if let Err(e) = slint::invoke_from_event_loop(move || complete(id, result)) {
                tracing::warn!("background job {id}: event loop gone: {e}");
            }
        });
    if let Err(e) = spawned {
        tracing::error!("background job {id}: thread spawn failed: {e}");
        complete(id, None);
    }
}

/// The user's action did not happen: say so (the details are in the log).
fn show_failure(ctx: &Rc<UiContext>) {
    if let Some(dialog) = modal::message(ctx, texts::ERROR_TITLE, texts::JOB_FAILED) {
        LAST_FAILURE.with(|d| *d.borrow_mut() = Some(dialog.as_weak()));
    }
}

fn complete(id: u64, result: Option<Box<dyn Any + Send>>) {
    let completion = PENDING.with(|p| p.borrow_mut().remove(&id));
    if let Some(completion) = completion {
        completion(result);
    }
}
