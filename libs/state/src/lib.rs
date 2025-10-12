mod misc;
mod persalarms;
mod settings;
mod sync;
pub mod util;

use anyhow::Context;
use chrono::NaiveDateTime;
use eventix_ical::{
    col::{CalDir, CalStore},
    objects::{EventLike, UpdatableEventLike},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::Mutex;
use tracing::debug;
use xdg::BaseDirectories;

pub use persalarms::{PersonalAlarms, PersonalCalendarAlarms};
pub use settings::{CalendarAlarmType, CalendarSettings, EmailAccount, Settings};
pub use sync::{SyncCalResult, SyncResult, Syncer};

pub type EventixState = Arc<Mutex<State>>;

pub struct State {
    xdg: Arc<BaseDirectories>,
    store: CalStore,
    personal_alarms: PersonalAlarms,
    settings: settings::Settings,
    misc: misc::Misc,
    last_reload: NaiveDateTime,
}

impl State {
    pub fn new(xdg: Arc<BaseDirectories>) -> anyhow::Result<Self> {
        let settings = settings::Settings::load_from_file(&xdg).context("load settings")?;

        let personal_alarms = PersonalAlarms::new_from_dir(&xdg).context("load personal alarms")?;

        let misc = misc::Misc::load_from_file(&xdg).context("load misc state")?;

        let mut store = CalStore::default();
        for (col_id, col) in settings.collections().iter() {
            for (cal_id, cal) in col.calendars() {
                let cal_id: Arc<String> = Arc::from(cal_id.clone());
                let col_path = col.path(&xdg, col_id);
                let path = col_path.join(cal.folder());
                let mut dir = if path.exists() {
                    CalDir::new_from_dir(cal_id.clone(), path.clone(), cal.name().clone())
                        .with_context(|| {
                            format!(
                                "Loading calendar {} from '{}' failed",
                                cal_id,
                                path.to_str().unwrap()
                            )
                        })?
                } else {
                    tracing::warn!(
                        "Creating empty calendar '{}' from non-existing directory {}",
                        cal_id,
                        path.to_str().unwrap()
                    );
                    CalDir::new_empty(cal_id.clone(), path, cal.name().clone())
                };

                // workaround for a bug in Exchange/davmail: apparently, Exchange sends events with
                // attendees, but without organizer to davmail and davmail does not repair it. As this
                // seems to *only* happen if we are the organizer, we implicitly add ourself as an
                // organizer to these events.
                let organizer = col.build_organizer();
                if let Some(organizer) = organizer {
                    for comp in dir.files_mut().iter_mut().flat_map(|f| {
                        f.component_with_mut(|c| {
                            c.rid().is_none() && c.organizer().is_none() && c.attendees().is_some()
                        })
                    }) {
                        tracing::warn!(
                            "Making ourself the organizer of group-scheduled item {}",
                            comp.uid()
                        );
                        comp.set_organizer(Some(organizer.clone()));
                    }
                }

                store.add(dir);
            }
        }

        Ok(Self {
            xdg,
            settings,
            personal_alarms,
            store,
            misc,
            last_reload: chrono::Utc::now().naive_utc(),
        })
    }

    pub async fn reload(
        state: EventixState,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        // first reload the settings and personal alarms
        let changed = {
            let mut state = state.lock().await;

            let settings =
                settings::Settings::load_from_file(&state.xdg).context("load settings")?;
            let personal_alarms =
                PersonalAlarms::new_from_dir(&state.xdg).context("load personal alarms")?;
            let misc = misc::Misc::load_from_file(&state.xdg).context("load misc state")?;

            let changed = state.personal_alarms != personal_alarms || state.misc != misc;

            state.personal_alarms = personal_alarms;
            state.settings = settings;
            state.misc = misc;

            changed
        };

        // now synchronize and update the store
        let mut sync_res = sync::sync_all(state.clone(), auth_url).await?;
        sync_res.changed |= changed;

        // remember last reload
        state.lock().await.last_reload = chrono::Utc::now().naive_utc();

        Ok(sync_res)
    }

    pub fn xdg(&self) -> &BaseDirectories {
        &self.xdg
    }

    pub fn store(&self) -> &CalStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut CalStore {
        &mut self.store
    }

    pub fn store_and_alarms_mut(&mut self) -> (&mut CalStore, &mut PersonalAlarms) {
        (&mut self.store, &mut self.personal_alarms)
    }

    pub fn personal_alarms(&self) -> &PersonalAlarms {
        &self.personal_alarms
    }

    pub fn personal_alarms_mut(&mut self) -> &mut PersonalAlarms {
        &mut self.personal_alarms
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn misc(&self) -> &misc::Misc {
        &self.misc
    }

    pub fn misc_mut(&mut self) -> &mut misc::Misc {
        &mut self.misc
    }

    pub fn last_reload(&self) -> NaiveDateTime {
        self.last_reload
    }
}

pub fn load_from_file<D: DeserializeOwned>(filename: &PathBuf) -> anyhow::Result<D> {
    debug!("Reading from {:?}", filename);
    let mut file = File::options()
        .read(true)
        .open(filename)
        .context(format!("open {filename:?}"))?;
    let mut data = String::new();
    file.read_to_string(&mut data)
        .context(format!("read {filename:?}"))?;
    toml::from_str(&data).context(format!("parse {filename:?}"))
}

pub fn write_to_file<S: Serialize>(filename: &PathBuf, data: S) -> anyhow::Result<()> {
    debug!("Writing to {:?}", filename);
    let mut file = File::options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(filename)
        .context(format!("open {filename:?}"))?;
    file.write_all(
        toml::to_string(&data)
            .context(format!("serialize {filename:?}"))?
            .as_bytes(),
    )
    .context(format!("write {filename:?}"))?;
    Ok(())
}
