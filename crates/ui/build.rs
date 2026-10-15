//! Compiles `ui/app.slint` with the light-only Fluent style (`fluent-light`): crm has
//! no dark mode, so every window is light whatever the operating system's scheme.
//! Debug builds (tests) also emit the element debug info that the headless
//! `ElementHandle` queries need.

fn main() {
    let debug = std::env::var("PROFILE").as_deref() == Ok("debug");
    slint_build::compile_with_config(
        "ui/app.slint",
        slint_build::CompilerConfiguration::new()
            .with_style("fluent-light".into())
            .with_debug_info(debug),
    )
    .expect("Slint UI compilation failed");
}
