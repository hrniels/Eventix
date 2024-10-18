use anyhow::anyhow;
use chrono::DateTime;
use chrono_tz::Tz;
use std::{
    fs::{read_dir, File},
    io::Read,
    path::PathBuf,
};

use super::{
    calendar::{Calendar, Component},
    CalItem, Id,
};

pub struct CalSource {
    id: Id,
    path: PathBuf,
    items: Vec<CalItem>,
}

impl Default for CalSource {
    fn default() -> Self {
        Self {
            id: super::generate_id(),
            path: PathBuf::default(),
            items: Vec::new(),
        }
    }
}

impl CalSource {
    pub fn new_from_dir(path: PathBuf) -> Result<Self, anyhow::Error> {
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

        Ok(Self { id, path, items })
    }

    pub fn id(&self) -> Id {
        self.id
    }

    pub fn add(&mut self, item: CalItem) {
        self.items.push(item);
    }

    pub fn items(&self) -> &[CalItem] {
        &self.items
    }

    pub fn items_within(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
    ) -> impl Iterator<Item = (&Component, DateTime<Tz>)> {
        self.items
            .iter()
            .map(move |i| i.items_within(start, end))
            .flatten()
    }
}
