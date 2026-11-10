// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{env, fs, io, path::PathBuf};

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    let app_id = if env::var("PROFILE").expect("Cargo sets PROFILE") == "debug" {
        "io.github.hrniels.Eventix-debug"
    } else {
        "io.github.hrniels.Eventix"
    };
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    fs::write(
        out.join("app_id.rs"),
        format!("const APP_ID: &str = {app_id:?};\n"),
    )
}
