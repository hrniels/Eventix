use anyhow::Context;
use chrono::NaiveDateTime;
use ical::objects::CalCompType;
use once_cell::sync::Lazy;
use tokio::sync::Mutex;

use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{Read, Write},
};

use serde::{Deserialize, Serialize};

use crate::state::State;

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(rename = "calendar")]
    pub calendars: BTreeMap<String, Calendar>,
    pub last_alarm_check: Option<NaiveDateTime>,
    pub last_calendar: HashMap<CalCompType, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Calendar {
    pub path: String,
    pub name: String,
    pub disabled: Option<bool>,
    pub fgcolor: String,
    pub bgcolor: String,
    pub types: Option<Vec<CalCompType>>,
}

const FILENAME: &str = "settings.toml";
static MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

impl Settings {
    pub async fn new_from_state(state: State) -> Self {
        let calendars = {
            let mut calendars = BTreeMap::new();
            let (store, disabled) = state.acquire_store_and_disabled().await;
            for source in store.sources() {
                calendars.insert(
                    source.id().to_string(),
                    Calendar {
                        path: source.path().to_str().unwrap().to_string(),
                        name: source.name().to_string(),
                        disabled: Some(disabled.contains(source.id())),
                        fgcolor: source
                            .props()
                            .get(&String::from("fgcolor"))
                            .unwrap()
                            .clone(),
                        bgcolor: source
                            .props()
                            .get(&String::from("bgcolor"))
                            .unwrap()
                            .clone(),
                        types: source
                            .props()
                            .get(&String::from("types"))
                            .map(|ty| serde_json::from_str(ty).unwrap()),
                    },
                );
            }
            calendars
        };
        let last_alarm_check = *state.last_alarm_check().lock().await;
        let last_calendar = state.last_calendar().lock().await.clone();

        Self {
            calendars,
            last_alarm_check: Some(last_alarm_check),
            last_calendar,
        }
    }

    pub async fn load_from_file() -> anyhow::Result<Self> {
        // ensure that reads/writes to this file do not happen in parallel
        let _guard = MUTEX.lock().await;
        let mut file = File::options()
            .read(true)
            .open(FILENAME)
            .context(format!("open {}", FILENAME))?;
        let mut sources = String::new();
        file.read_to_string(&mut sources)
            .context(format!("read {}", FILENAME))?;
        Ok(toml::from_str(&sources).context(format!("parse {}", FILENAME))?)
    }

    pub async fn write_to_file(&self) -> anyhow::Result<()> {
        let _guard = MUTEX.lock().await;
        let mut file = File::options()
            .write(true)
            .create(true)
            .truncate(true)
            .open(FILENAME)
            .context(format!("open {}", FILENAME))?;
        file.write_all(
            toml::to_string(self)
                .context("serialize settings")?
                .as_bytes(),
        )
        .context("write settings")?;
        Ok(())
    }
}
