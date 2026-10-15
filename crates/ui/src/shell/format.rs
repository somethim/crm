//! Formatting for features filling Slint models: thin re-exports of
//! `crm_domain::format` (the only place values are formatted), plus the
//! `DateParse` global used by the shared `DateField` and its calendar popup.

pub use crm_services::domain::format::*;
pub use crm_services::domain::time::{tirane, today};

use super::calendar;
use crate::{CalendarDay, ParsedDate};
use crm_services::domain::format as f;
use jiff::civil::Date;
use slint::{ModelRc, VecModel};
use std::rc::Rc;

/// `DateParse.parse`: user input to `{ok, iso, display}`.
pub fn parse_date_field(text: &str) -> ParsedDate {
    match f::parse_date(text) {
        Ok(date) => ParsedDate {
            ok: true,
            iso: date.to_string().into(),
            display: f::date(date).into(),
        },
        Err(_) => ParsedDate {
            ok: false,
            iso: Default::default(),
            display: text.into(),
        },
    }
}

/// `DateParse.shift`: a valid date moved by `days`.
pub fn shift_date_field(text: &str, days: i32) -> ParsedDate {
    match f::parse_date(text) {
        Ok(date) => match date.checked_add(jiff::Span::new().days(days)) {
            Ok(moved) => ParsedDate {
                ok: true,
                iso: moved.to_string().into(),
                display: f::date(moved).into(),
            },
            Err(_) => parse_date_field(text),
        },
        Err(_) => parse_date_field(text),
    }
}

/// A valid date as `{ok, iso, display}`.
fn parsed(date: Date) -> ParsedDate {
    ParsedDate {
        ok: true,
        iso: date.to_string().into(),
        display: f::date(date).into(),
    }
}

/// A calendar bound: empty or invalid text is no bound.
fn bound(text: &str) -> Option<Date> {
    f::parse_date(text).ok()
}

/// `DateParse.shift-months`: a valid date moved by whole months (the day
/// clamped to the month's length).
pub fn shift_months_field(text: &str, months: i32) -> ParsedDate {
    match f::parse_date(text) {
        Ok(date) => parsed(calendar::add_months(date, months)),
        Err(_) => parse_date_field(text),
    }
}

/// `DateParse.calendar-start`: the day the calendar opens on: the field's date,
/// or today when it has none, moved into `min ..= max`.
pub fn calendar_start(text: &str, min: &str, max: &str) -> ParsedDate {
    let date = bound(text).unwrap_or_else(today);
    parsed(calendar::clamp(date, bound(min), bound(max)))
}

/// `DateParse.today`: today's date.
pub fn today_field() -> ParsedDate {
    parsed(today())
}

/// `DateParse.allowed`: whether the date `text` lies in `min ..= max`.
pub fn allowed_field(text: &str, min: &str, max: &str) -> bool {
    bound(text).is_some_and(|date| calendar::allowed(date, bound(min), bound(max)))
}

/// `DateParse.month-title`: "Tetor 2026" for the month of `cursor`.
pub fn month_title_field(cursor: &str) -> slint::SharedString {
    bound(cursor).map(f::month_year).unwrap_or_default().into()
}

/// `DateParse.month-grid`: the 42 cells of the month containing `cursor`.
pub fn month_grid_field(
    cursor: &str,
    selected: &str,
    min: &str,
    max: &str,
) -> ModelRc<CalendarDay> {
    let month = bound(cursor).unwrap_or_else(today);
    let cells = calendar::month_grid(month, bound(selected), bound(min), bound(max), today());
    let days: Vec<CalendarDay> = cells
        .into_iter()
        .map(|c| CalendarDay {
            iso: c.date.to_string().into(),
            display: f::date(c.date).into(),
            day: i32::from(c.date.day()),
            in_month: c.in_month,
            today: c.today,
            selected: c.selected,
            enabled: c.enabled,
        })
        .collect();
    ModelRc::from(Rc::new(VecModel::from(days)))
}

/// Installs the `DateParse` handlers on a window's global (every window that
/// uses `DateField` needs this once): `install_date_parse!(window)`.
#[macro_export]
macro_rules! install_date_parse {
    ($window:expr) => {{
        use ::slint::ComponentHandle as _;
        let parse = $window.global::<$crate::DateParse>();
        parse.on_parse(|text| $crate::shell::format::parse_date_field(&text));
        parse.on_shift(|text, days| $crate::shell::format::shift_date_field(&text, days));
        parse.on_shift_months(|text, months| {
            $crate::shell::format::shift_months_field(&text, months)
        });
        parse.on_calendar_start(|text, min, max| {
            $crate::shell::format::calendar_start(&text, &min, &max)
        });
        parse.on_today($crate::shell::format::today_field);
        parse.on_allowed(|text, min, max| $crate::shell::format::allowed_field(&text, &min, &max));
        parse.on_month_title(|cursor| $crate::shell::format::month_title_field(&cursor));
        parse.on_month_grid(|cursor, selected, min, max| {
            $crate::shell::format::month_grid_field(&cursor, &selected, &min, &max)
        });
    }};
}
