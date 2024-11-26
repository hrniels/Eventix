use chrono::DateTime;
use chrono_tz::Tz;
use std::fmt::Display;
use std::fs::{read_dir, File};
use std::io::Read;
use std::path::PathBuf;

use crate::col::{CalItem, ColError, Id, Occurrence};
use crate::objects::{CalComponent, CalDate, Calendar};

#[derive(Debug)]
pub struct CalSource {
    id: Id,
    path: PathBuf,
    name: String,
    items: Vec<CalItem>,
}

impl Display for CalSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl PartialEq for CalSource {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.items == other.items
    }
}
impl Eq for CalSource {}

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
    pub fn new_from_dir(path: PathBuf, name: String) -> Result<Self, ColError> {
        let id = super::generate_id();

        let mut items = Vec::new();
        let dir_items = read_dir(path.as_path()).map_err(|e| ColError::ReadDir(path.clone(), e))?;
        for entry in dir_items {
            let filename = entry
                .map_err(|e| ColError::ReadDir(path.clone(), e))?
                .path();

            let mut input = String::new();
            File::open(filename.as_path())
                .map_err(|e| ColError::FileOpen(filename.clone(), e))?
                .read_to_string(&mut input)
                .map_err(|e| ColError::FileRead(filename.clone(), e))?;

            let cal = input
                .parse::<Calendar>()
                .map_err(|e| ColError::FileParse(filename.clone(), e))?;
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

    pub fn item_by_id<S: AsRef<str>>(&self, uid: S) -> Option<&CalItem> {
        let uid_ref = uid.as_ref();
        self.items.iter().find(|i| i.contains_uid(uid_ref))
    }

    pub fn item_by_id_mut<S: AsRef<str>>(&mut self, uid: S) -> Option<&mut CalItem> {
        let uid_ref = uid.as_ref();
        self.items.iter_mut().find(|i| i.contains_uid(uid_ref))
    }

    pub fn next_alarm_occurrence(&self, start: DateTime<Tz>) -> Option<Occurrence<'_>> {
        self.items
            .iter()
            .filter_map(|i| i.next_alarm_occurrence(start))
            .min_by(|a, b| a.alarm_date().unwrap().cmp(&b.alarm_date().unwrap()))
    }

    pub fn occurrence_by_id<S: AsRef<str>>(
        &self,
        uid: S,
        rid: Option<&CalDate>,
        tz: &Tz,
    ) -> Option<Occurrence<'_>> {
        let uid_str = uid.as_ref();
        self.items
            .iter()
            .find_map(|i| i.occurrence_by_id(uid_str, rid, tz))
    }

    pub fn occurrences_within(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
    ) -> impl Iterator<Item = Occurrence<'_>> {
        self.filtered_occurrences_within(start, end, |_| true)
    }

    pub fn filtered_occurrences_within<F>(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
        filter: F,
    ) -> impl Iterator<Item = Occurrence<'_>>
    where
        F: Fn(&CalComponent) -> bool + Clone,
    {
        self.items
            .iter()
            .flat_map(move |i| i.filtered_occurrences_within(start, end, filter.clone()))
    }

    pub fn save(&self) -> Result<(), ColError> {
        for i in &self.items {
            i.save()?;
        }
        Ok(())
    }
}
