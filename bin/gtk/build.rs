// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{env, io, path::PathBuf, process::Command};

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=resources");
    let target = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"))
        .join("eventix.gresource");
    let status = Command::new("glib-compile-resources")
        .arg("resources/eventix.gresource.xml")
        .arg("--sourcedir=resources")
        .arg("--target")
        .arg(target)
        .status()?;
    if !status.success() {
        return Err(io::Error::other("compiling GTK resources failed"));
    }
    Ok(())
}
