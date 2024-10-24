use anyhow::anyhow;
use chrono::DateTime;
use chrono_tz::Tz;
use std::fs::{read_dir, File};
use std::io::Read;
use std::path::PathBuf;

use crate::col::{CalItem, Id, Occurrence};
use crate::objects::{CalComponent, CalDate, Calendar};

pub struct CalSource {
    id: Id,
    path: PathBuf,
    name: String,
    items: Vec<CalItem>,
}

impl Default for CalSource {
    fn default() -> Self {
        Self {
            id: super::generate_id(),
            path: PathBuf::default(),
            name: String::default(),
            items: Vec::new(),
        }
    }
}

impl CalSource {
    pub fn new_from_dir(path: PathBuf, name: String) -> Result<Self, anyhow::Error> {
        let id = super::generate_id();

        let mut items = Vec::new();
        for e in read_dir(path.as_path())? {
            let filename = e?.path();

            let mut input = String::new();
            File::open(filename.as_path())?.read_to_string(&mut input)?;

            let cal = input.parse::<Calendar>().map_err(|e| {
                anyhow!("Parsing calendar in {:?} failed: {}", filename.as_path(), e)
            })?;
            let cal = CalItem::new(id, filename, cal);
            items.push(cal);
        }

        Ok(Self {
            id,
            path,
            name,
            items,
        })
    }

    pub fn id(&self) -> Id {
        self.id
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn add(&mut self, item: CalItem) {
        self.items.push(item);
    }

    pub fn items(&self) -> &[CalItem] {
        &self.items
    }

    pub fn occurrence_by_id<S: AsRef<str>>(
        &self,
        uid: S,
        rid: &CalDate,
        tz: &Tz,
    ) -> Option<Occurrence<'_>> {
        let uid_str = uid.as_ref();
        self.items
            .iter()
            .find_map(|c| c.occurrence_by_id(uid_str, rid, tz))
    }

    pub fn occurrences_within(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
    ) -> impl Iterator<Item = Occurrence<'_>> {
        self.items
            .iter()
            .flat_map(move |i| i.occurrences_within(start, end))
    }
}
