//! `crm.exe`: the portable desktop CRM.
//!
//! Release builds use the Windows GUI subsystem (no console window); debug
//! builds keep the console for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod instance;
mod logging;
mod startup;

fn main() -> std::process::ExitCode {
    startup::run()
}
