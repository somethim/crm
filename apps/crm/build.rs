//! Embeds the icon, the application manifest and VERSIONINFO into `crm.exe`.
//! Does nothing unless the target OS is Windows.

use std::{env, fs, path::PathBuf};

fn main() {
    let resources = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("resources");
    for file in ["crm.rc.in", "crm.ico", "crm.manifest"] {
        println!("cargo:rerun-if-changed={}", resources.join(file).display());
    }

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts: Vec<&str> = version.split(['.', '-', '+']).take(3).collect();
    parts.resize(3, "0");
    let version_comma = format!("{},0", parts.join(","));

    // rc paths: absolute, forward slashes (valid for both rc.exe and llvm-rc).
    let rc_path = |file: &str| {
        resources
            .join(file)
            .display()
            .to_string()
            .replace('\\', "/")
    };

    let template = fs::read_to_string(resources.join("crm.rc.in")).unwrap();
    let rendered = template
        .replace("@ICON@", &rc_path("crm.ico"))
        .replace("@MANIFEST@", &rc_path("crm.manifest"))
        .replace("@VERSION_COMMA@", &version_comma)
        .replace("@VERSION@", &version);

    let out_rc = PathBuf::from(env::var("OUT_DIR").unwrap()).join("crm.rc");
    fs::write(&out_rc, rendered).unwrap();

    embed_resource::compile(&out_rc, embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
