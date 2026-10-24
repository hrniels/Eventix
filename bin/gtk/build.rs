// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs, io};

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=resources");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let app_id = if env::var("PROFILE").expect("Cargo sets PROFILE") == "debug" {
        "io.github.hrniels.Eventix-debug"
    } else {
        "io.github.hrniels.Eventix"
    };
    let resource_prefix = format!("/{}/gtk", app_id.replace('.', "/"));
    let resources = out_dir.join("resources");
    prepare_resources(Path::new("resources"), &resources, &resource_prefix)?;

    let mut generated = format!(
        "const APP_ID: &str = {app_id:?};\n\
         const RESOURCE_PREFIX: &str = {resource_prefix:?};\n\
         macro_rules! resource_path {{\n"
    );
    let manifest = fs::read_to_string(resources.join("eventix.gresource.xml"))?;
    let files: Vec<_> = manifest
        .lines()
        .filter_map(|line| line.trim().strip_prefix("<file>")?.strip_suffix("</file>"))
        .collect();
    for file in &files {
        let path = format!("{resource_prefix}/{file}");
        generated.push_str(&format!("    ({file:?}) => {{ {path:?} }};\n"));
    }
    generated.push_str("}\nmacro_rules! resource_template {\n");
    // CompositeTemplate requires a literal resource path in its attribute.
    for file in files.iter().filter(|file| file.ends_with(".ui")) {
        let path = format!("{resource_prefix}/{file}");
        generated.push_str(&format!(
            "    ({file:?}, $item:item) => {{\n\
                 #[derive(Default, gtk::CompositeTemplate)]\n\
                 #[template(resource = {path:?})]\n\
                 $item\n\
             }};\n"
        ));
    }
    generated.push_str("}\n");
    fs::write(out_dir.join("resources.rs"), generated)?;

    let target = out_dir.join("eventix.gresource");
    let status = Command::new("glib-compile-resources")
        .arg(resources.join("eventix.gresource.xml"))
        .arg("--sourcedir")
        .arg(&resources)
        .arg("--target")
        .arg(target)
        .status()?;
    if !status.success() {
        return Err(io::Error::other("compiling GTK resources failed"));
    }

    Ok(())
}

/// Copies resources while expanding the build-specific resource prefix in XML files.
fn prepare_resources(source: &Path, target: &Path, prefix: &str) -> io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source = entry.path();
        let target = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            prepare_resources(&source, &target, prefix)?;
        } else if matches!(
            source.extension().and_then(|ext| ext.to_str()),
            Some("ui" | "xml")
        ) {
            let content = fs::read_to_string(source)?.replace("@RESOURCE_PREFIX@", prefix);
            fs::write(target, content)?;
        } else {
            fs::copy(source, target)?;
        }
    }
    Ok(())
}
