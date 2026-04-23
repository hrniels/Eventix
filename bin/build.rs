// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::File;
use std::io::Read;
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

    let manifest_dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).to_path_buf();
    let workspace_root = fs::canonicalize(manifest_dir.join("../.."))
        .expect("Failed to canonicalize workspace root");
    let git_dir = workspace_root.join(".git");
    let git_head = git_dir.join("HEAD");
    println!("cargo:rerun-if-changed={}", git_head.display());

    let mut head = String::new();
    File::open(&git_head)
        .and_then(|mut file| file.read_to_string(&mut head))
        .expect("Failed to read .git/HEAD");
    if let Some(reference) = head.strip_prefix("ref: ") {
        println!(
            "cargo:rerun-if-changed={}",
            git_dir.join(reference.trim()).display()
        );
    }

    let app_id = if env::var("PROFILE").unwrap() == "debug" {
        "com.github.hrniels.Eventix-debug"
    } else {
        "com.github.hrniels.Eventix"
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

        println!(
            "cargo:rerun-if-changed={}",
            icons_path.join(format!("{icon}.png")).display()
        );
    }
}
