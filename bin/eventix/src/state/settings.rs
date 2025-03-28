use ical::objects::{CalCompType, CalOrganizer};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

const FILENAME: &str = "settings.toml";

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(rename = "calendar")]
    calendars: BTreeMap<String, CalendarSettings>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            calendars: BTreeMap::default(),
        }
    }
}

impl Settings {
    pub fn calendars(&self) -> &BTreeMap<String, CalendarSettings> {
        &self.calendars
    }

    pub fn calendar(&self, id: &String) -> Option<&CalendarSettings> {
        self.calendars.get(id)
    }

    pub fn emails(&self) -> HashMap<String, String> {
        let mut res = HashMap::new();
        for (id, settings) in &self.calendars {
            if let Some(email) = settings.email() {
                res.insert(id.clone(), email.pretty_name());
            }
        }
        res
    }

    pub fn load_from_file() -> anyhow::Result<Self> {
        super::load_from_file(&FILENAME.into())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmailAccount {
    name: String,
    address: String,
}

impl EmailAccount {
    pub fn pretty_name(&self) -> String {
        format!("{} <{}>", self.name, self.address)
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn address(&self) -> &String {
        &self.address
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CalendarSettings {
    path: String,
    name: String,
    email: Option<EmailAccount>,
    fgcolor: String,
    bgcolor: String,
    types: Vec<CalCompType>,
    syncer: Syncer,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Syncer {
    FileSystem,
    VDirSyncer {
        cmd: Vec<String>,
        local_name: String,
    },
}

impl CalendarSettings {
    pub fn path(&self) -> &String {
        &self.path
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn email(&self) -> Option<&EmailAccount> {
        self.email.as_ref()
    }

    pub fn build_organizer(&self) -> Option<CalOrganizer> {
        self.email()
            .map(|em| CalOrganizer::new_named(em.name().to_string(), em.address().to_string()))
    }

    pub fn types(&self) -> &[CalCompType] {
        &self.types
    }

    pub fn fgcolor(&self) -> &String {
        &self.fgcolor
    }

    pub fn bgcolor(&self) -> &String {
        &self.bgcolor
    }

    pub fn syncer(&self) -> &Syncer {
        &self.syncer
    }
}
