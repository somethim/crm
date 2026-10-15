//! Receipt preview, PNG save and printing (task 012). The orders workspace's
//! Printo (`OrdersBridge.print-current`) calls [`open_preview`] for the active
//! saved invoice; the receipt itself is rendered by `crm-reports`.

pub mod data;
pub mod preview;
pub mod texts;

#[cfg(test)]
mod tests;

pub use preview::{Preview, open_preview};
