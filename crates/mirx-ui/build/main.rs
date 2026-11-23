//! Windows resources for `mirx-ui.exe`: mirx's icon (drawn by `icon.rs`),
//! the toolbar's and the windows' glyphs (`glyphs.rs`), `VERSIONINFO` and the
//! application manifest, compiled by windres into one
//! COFF object that is linked into every bin target (the exe and its unit-test
//! harness, whose tests read the resources back).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod glyphs;
mod ico;
mod icon;
mod raster;
mod resources;

fn main() {
    println!("cargo:rerun-if-changed=build");
    println!("cargo:rerun-if-changed=res");
    // Host builds (the workspace tests and host clippy) are not Windows
    // programs and carry no resources.
    if env::var("CARGO_CFG_TARGET_OS").expect("cargo sets CARGO_CFG_TARGET_OS") != "windows" {
        return;
    }
    let target = env::var("TARGET").expect("cargo sets TARGET");
    assert_eq!(
        target, "x86_64-pc-windows-gnu",
        "mirx's resources are compiled with x86_64-w64-mingw32-windres, the windows-gnu toolchain's"
    );

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));

    let icon_path = out_dir.join("mirx.ico");
    fs::write(&icon_path, ico::encode(&icon::render_all())).expect("write mirx.ico");

    let glyph_icons = glyphs::GLYPHS
        .iter()
        .map(|glyph| {
            let path = out_dir.join(format!("glyph-{}.ico", glyph.stem));
            fs::write(&path, ico::encode(&glyph.render_all())).expect("write a glyph icon");
            (glyph.id, path)
        })
        .collect::<Vec<_>>();

    let rc_path = out_dir.join("mirx-ui.rc");
    let rc = resources::rc_source(&resources::RcInputs {
        icon_path: &icon_path,
        glyph_icons: &glyph_icons,
        manifest_path: &manifest_dir.join("res").join("mirx-ui.exe.manifest"),
        version: resources::Version::from_cargo_env(),
    });
    fs::write(&rc_path, rc).expect("write mirx-ui.rc");

    let object_path = out_dir.join("mirx-ui-res.o");
    compile_resources(&rc_path, &object_path);
    // By path, not as a static library: ld would drop an archive member that
    // defines no referenced symbol, and the resources with it.
    println!("cargo:rustc-link-arg-bins={}", object_path.display());
}

fn compile_resources(rc_path: &Path, object_path: &Path) {
    let output = Command::new("x86_64-w64-mingw32-windres")
        .args([
            "--input-format=rc",
            "--output-format=coff",
            "--codepage=65001",
            "-i",
        ])
        .arg(rc_path)
        .arg("-o")
        .arg(object_path)
        .output()
        .expect("run x86_64-w64-mingw32-windres (the devshell provides it)");
    assert!(
        output.status.success(),
        "windres failed ({}):\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}
