// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::OnceLock;

use ashpd::{WindowIdentifier, desktop::background::Background};
use tokio::runtime::{Builder, Runtime};

static RUNTIME: OnceLock<Result<Runtime, std::io::Error>> = OnceLock::new();

/// Updates login autostart through the portal and checks the desktop's decision.
pub async fn request(window: &gtk::Window, enabled: bool, reason: &str) -> anyhow::Result<()> {
    let identifier = WindowIdentifier::from_native(window).await;
    // Keep the exported GTK handle on its owning thread until the portal has finished.
    let parent = identifier
        .as_ref()
        .map(|identifier| identifier.to_string().parse().map(WindowIdentifier::Raw))
        .transpose()?;
    let reason = reason.to_owned();
    let runtime = RUNTIME
        .get_or_init(|| {
            Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
        })
        .as_ref()
        .map_err(|error| anyhow::anyhow!("Could not start portal runtime: {error}"))?;
    let response = runtime
        .spawn(async move {
            Background::request()
                .identifier(parent)
                .reason(reason.as_str())
                .auto_start(enabled)
                .command(["eventix-gtk", "--background"])
                .send()
                .await?
                .response()
        })
        .await??;
    drop(identifier);
    check_response(enabled, response.auto_start(), response.run_in_background())
}

fn check_response(enabled: bool, autostart: bool, background: bool) -> anyhow::Result<()> {
    anyhow::ensure!(autostart == enabled, "Autostart request was denied");
    anyhow::ensure!(
        !enabled || background,
        "Background permission required for tray-only startup was denied"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_response;

    #[test]
    fn enabling_requires_both_permissions_but_disabling_only_requires_autostart_off() {
        assert!(check_response(true, true, true).is_ok());
        assert!(check_response(true, false, true).is_err());
        assert!(check_response(true, true, false).is_err());
        assert!(check_response(true, false, false).is_err());
        assert!(check_response(false, false, true).is_ok());
        assert!(check_response(false, false, false).is_ok());
        assert!(check_response(false, true, true).is_err());
    }
}
