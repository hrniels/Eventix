use chrono::DateTime;
use chrono_tz::Tz;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs::{read_dir, File};
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use crate::col::{CalFile, ColError, Occurrence};
use crate::objects::{CalComponent, CalDate, Calendar};

#[derive(Default, Debug)]
pub struct CalDir {
    id: Arc<String>,
    path: PathBuf,
    name: String,
    props: HashMap<String, String>,
    files: Vec<CalFile>,
}

impl Display for CalDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl PartialEq for CalDir {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.files == other.files
    }
}
impl Eq for CalDir {}

impl CalDir {
    pub fn new_from_dir(
        id: Arc<String>,
        path: PathBuf,
        name: String,
        props: HashMap<String, String>,
    ) -> Result<Self, ColError> {
        let mut files = Vec::new();
        let dir_files = read_dir(path.as_path()).map_err(|e| ColError::ReadDir(path.clone(), e))?;
        for entry in dir_files {
            let entry = entry.map_err(|e| ColError::ReadDir(path.clone(), e))?;
            if !entry
                .file_type()
                .map_err(|e| ColError::FileType(path.clone(), e))?
                .is_file()
            {
                continue;
            }

            let filename = entry.path();

            let mut input = String::new();
            File::open(filename.as_path())
                .map_err(|e| ColError::FileOpen(filename.clone(), e))?
                .read_to_string(&mut input)
                .map_err(|e| ColError::FileRead(filename.clone(), e))?;

            let cal = input
                .parse::<Calendar>()
                .map_err(|e| ColError::FileParse(filename.clone(), e))?;
            let file = CalFile::new(id.clone(), filename, cal);
            files.push(file);
        }

        Ok(Self {
            id,
            path,
            name,
            props,
            files,
        })
    }

    pub fn id(&self) -> &Arc<String> {
        &self.id
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn props(&self) -> &HashMap<String, String> {
        &self.props
    }

    pub fn files(&self) -> &[CalFile] {
        &self.files
    }

    pub fn file_by_id<S: AsRef<str>>(&self, uid: S) -> Option<&CalFile> {
        let uid_ref = uid.as_ref();
        self.files.iter().find(|i| i.contains_uid(uid_ref))
    }

    pub fn file_by_id_mut<S: AsRef<str>>(&mut self, uid: S) -> Option<&mut CalFile> {
        let uid_ref = uid.as_ref();
        self.files.iter_mut().find(|i| i.contains_uid(uid_ref))
    }

    pub fn add_file(&mut self, file: CalFile) {
        self.files.push(file);
    }

    pub fn due_alarms_within(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
    ) -> impl Iterator<Item = Occurrence<'_>> {
        self.files
            .iter()
            .flat_map(move |i| i.due_alarms_within(start, end))
    }

    pub fn occurrence_by_id<S: AsRef<str>>(
        &self,
        uid: S,
        rid: Option<&CalDate>,
        tz: &Tz,
    ) -> Option<Occurrence<'_>> {
        let uid_str = uid.as_ref();
        self.files
            .iter()
            .find_map(|i| i.occurrence_by_id(uid_str, rid, tz))
    }

    pub fn occurrences_within<F>(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
        filter: F,
    ) -> impl Iterator<Item = Occurrence<'_>>
    where
        F: Fn(&CalComponent) -> bool + Clone,
    {
        self.files
            .iter()
            .flat_map(move |i| i.occurrences_within(start, end, filter.clone()))
    }

    pub fn delete_by_uid<S: AsRef<str>>(&mut self, uid: S) -> Result<(), ColError> {
        let file = self.file_by_id_mut(&uid).unwrap();
        file.delete_by_uid(uid);
        if file.components().is_empty() {
            let path = file.path().clone();
            self.delete_file(&path).map(|_| ())
        } else {
            file.save()
        }
    }

    pub(crate) fn delete_file(&mut self, path: &PathBuf) -> Result<CalFile, ColError> {
        let idx = self
            .files
            .iter()
            .position(|i| i.path() == path)
            .ok_or_else(|| ColError::FileNotFound(path.clone()))?;
        let mut file = self.files.remove(idx);
        file.remove()?;
        Ok(file)
    }

    pub fn save(&self) -> Result<(), ColError> {
        for i in &self.files {
            i.save()?;
        }
        Ok(())
    }
}
