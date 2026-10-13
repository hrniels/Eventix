// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Background calendar loading; this module never accesses GTK objects.

use std::{sync::Arc, thread};

use chrono::{NaiveDate, Utc};
use eventix_view::{MonthSnapshot, WeekSnapshot};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum View {
    #[default]
    Week,
    Month,
}

pub(crate) enum Snapshot {
    Week(WeekSnapshot),
    Month(MonthSnapshot),
}

pub(crate) struct Request {
    pub generation: u64,
    pub date: Option<NaiveDate>,
    pub reload: bool,
    pub view: View,
}

pub(crate) struct Response {
    pub generation: u64,
    pub result: Result<(Snapshot, String), String>,
}

/// Owns the request channel, closing the worker when the page stops or is dropped.
pub(crate) struct CalendarLoader {
    requests: async_channel::Sender<Request>,
}

impl CalendarLoader {
    pub fn start(prefix: String) -> (Self, async_channel::Receiver<Response>) {
        let (requests, rx) = async_channel::unbounded::<Request>();
        let (results, responses) = async_channel::unbounded();
        thread::spawn(move || {
            let xdg = Arc::new(xdg::BaseDirectories::with_prefix(prefix));
            let mut state = None;
            while let Ok(mut request) = rx.recv_blocking() {
                // Keep the newest date without losing any queued reload request.
                while let Ok(newer) = rx.try_recv() {
                    let reload = request.reload || newer.reload;
                    request = newer;
                    request.reload = reload;
                }
                let result = (|| -> anyhow::Result<(Snapshot, String)> {
                    if state.is_none() || request.reload {
                        state = Some(load_local_state(xdg.clone())?);
                    }
                    let state = state.as_ref().expect("state was successfully loaded");
                    let date = request
                        .date
                        .unwrap_or_else(|| Utc::now().with_timezone(state.timezone()).date_naive());
                    let snapshot = match request.view {
                        View::Week => Snapshot::Week(WeekSnapshot::load(state, date)?),
                        View::Month => Snapshot::Month(MonthSnapshot::load(state, date)?),
                    };
                    let config = xdg
                        .find_config_file("settings.toml")
                        .or_else(|| xdg.get_config_file("settings.toml"))
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    let data = xdg
                        .get_data_home()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    let source = format!(
                        "{}\n{}",
                        formatx::formatx!(
                            state.locale().translate("Calendar configuration: {}"),
                            config
                        )
                        .unwrap(),
                        formatx::formatx!(state.locale().translate("Calendar data: {}"), data)
                            .unwrap(),
                    );
                    Ok((snapshot, source))
                })()
                .map_err(|e| format!("{e:#}"));
                if results
                    .send_blocking(Response {
                        generation: request.generation,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        (Self { requests }, responses)
    }

    pub fn request(&self, request: Request) {
        let _ = self.requests.try_send(request);
    }
}

impl Drop for CalendarLoader {
    fn drop(&mut self) {
        self.requests.close();
    }
}

/// Loads an existing local profile, reporting missing settings instead of an empty calendar.
fn load_local_state(xdg: Arc<xdg::BaseDirectories>) -> anyhow::Result<eventix_state::State> {
    let state = eventix_state::State::new(xdg.clone())?;
    if xdg.find_config_file("settings.toml").is_none() {
        let expected = xdg
            .get_config_file("settings.toml")
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "settings.toml".into());
        anyhow::bail!(
            formatx::formatx!(state.locale().translate(
            "No calendar configuration found at {}. Check XDG_CONFIG_HOME and --data-prefix."
        ), expected)
            .unwrap()
        );
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc};

    use super::load_local_state;

    #[test]
    fn missing_profile_reports_expected_path_but_explicit_empty_profile_loads() {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(tmp.path(), tmp.path()));
        let error = load_local_state(xdg.clone())
            .err()
            .expect("missing profile must fail");
        assert!(
            error
                .to_string()
                .contains(&tmp.path().join("settings.toml").display().to_string())
        );
        let settings = eventix_state::Settings::new(tmp.path().join("settings.toml"));
        settings.write_to_file().unwrap();
        let before = fs::read(tmp.path().join("settings.toml")).unwrap();
        let state = load_local_state(xdg).unwrap();
        assert!(state.store().directories().is_empty());
        assert_eq!(fs::read(tmp.path().join("settings.toml")).unwrap(), before);
    }
}
