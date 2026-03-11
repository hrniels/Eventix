// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::{env, fs};

fn main() {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse HEAD");
    let git_hash = String::from_utf8(output.stdout)
        .expect("Invalid UTF-8 in git output")
        .trim()
        .to_string();
    println!("cargo:rustc-env=GIT_HASH={git_hash}");

    let app_id = if env::var("PROFILE").unwrap() == "debug" {
        "com.github.NilsTUD.Eventix-debug"
    } else {
        "com.github.NilsTUD.Eventix"
    };
    let icons = ["month", "week", "list", "event", "todo"];

    let icons_path = Path::new("../../data").join("icons");
    let icons_path = fs::canonicalize(&icons_path).expect("Failed to canonicalize icons path");

    let out_dir = env::var("OUT_DIR").unwrap();
    let path = Path::new(&out_dir).join("icons.rs");
    let mut f = File::options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .unwrap();

    writeln!(f, "pub const APP_ID: &str = \"{app_id}\";\n").unwrap();

    for icon in icons {
        writeln!(
            f,
            "pub const ICON_{}: &[u8] = include_bytes!(\"{}/{}.png\");",
            icon.to_string().to_uppercase(),
            icons_path.to_str().unwrap(),
            icon
        )
        .unwrap();

        println!("cargo:rerun-if-changed=../../../data/{app_id}/icons/{icon}.png",);
    }
}
