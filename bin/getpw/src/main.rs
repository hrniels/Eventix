// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use eventix_state::{decrypt_password, retrieve_portal_secret, Settings};
use xdg::BaseDirectories;

/// Simple tool to retrieve passwords from the Eventix settings.
#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    /// The collection id to retrieve the password for
    col_id: String,
}

include!(concat!(env!("OUT_DIR"), "/app_id.rs"));

fn main() -> Result<()> {
    let args = Args::parse();

    let xdg = Arc::new(BaseDirectories::with_prefix(APP_ID));
    let settings = Settings::load_from_file(&xdg).context("loading settings")?;

    let cols = settings.collections();
    let col = cols
        .get(&args.col_id)
        .ok_or_else(|| anyhow!("No collection {}", args.col_id))?;

    if let Some(encrypted) = col.syncer().password() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let secret = rt.block_on(async { retrieve_portal_secret().await })?;
        let password = decrypt_password(&secret, encrypted)?;
        print!("{}", password);
        Ok(())
    } else {
        Err(anyhow!("No password found for collection {}", args.col_id))
    }
}
