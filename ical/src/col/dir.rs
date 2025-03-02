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

/// A directory with calendar files.
///
/// A [`CalDir`] is a container for [`CalFile`] objects with additional properties. At first, each
/// directory has a unique id and a human-readable name. Furthermore, custom properties can be set.
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
    /// Creates a new directory from the given path.
    ///
    /// This method reads all files in the given directory and tries to parse them into a
    /// [`Calendar`]. These are added to the created [`CalDir`]. Note that it expects all files to
    /// be calendar files, but ignores directories.
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

    /// Returns the unique id of this directory.
    pub fn id(&self) -> &Arc<String> {
        &self.id
    }

    /// Returns the file system path of this directory.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Returns the human-readable name of this directory.
    pub fn name(&self) -> &String {
        &self.name
    }

    /// Returns the additional properties that have been set for this directory.
    pub fn props(&self) -> &HashMap<String, String> {
        &self.props
    }

    /// Returns a slice with all files in this directory.
    pub fn files(&self) -> &[CalFile] {
        &self.files
    }

    /// Returns a reference to the file that hosts the component with given uid.
    pub fn file_by_id<S: AsRef<str>>(&self, uid: S) -> Option<&CalFile> {
        let uid_ref = uid.as_ref();
        self.files.iter().find(|i| i.contains_uid(uid_ref))
    }

    /// Returns a mutable reference to the file that hosts the component with given uid.
    pub fn file_by_id_mut<S: AsRef<str>>(&mut self, uid: S) -> Option<&mut CalFile> {
        let uid_ref = uid.as_ref();
        self.files.iter_mut().find(|i| i.contains_uid(uid_ref))
    }

    /// Adds the given file to this directory.
    pub fn add_file(&mut self, file: CalFile) {
        self.files.push(file);
    }

    /// Returns a vector of occurrences whose alarm is due within the given time period.
    ///
    /// Note that excluded occurrences are not returned.
    pub fn due_alarms_within(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
    ) -> impl Iterator<Item = Occurrence<'_>> {
        self.files
            .iter()
            .flat_map(move |i| i.due_alarms_within(start, end))
    }

    /// Returns the occurrence with given uid/rid.
    ///
    /// If `rid` is `None`, this method simply returns the base component with the given uid as an
    /// [`Occurrence`], if it does exist. If `rid` is `Some`, it will determine the whether an
    /// overwrite for this specific date (given by the `rid`) exists and if so, it will be
    /// contained in the [`Occurrence`]. The timezone is used to create the date instances in the
    /// returned occurrence.
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

    /// Returns an iterator with all occurrences in the given period of time.
    ///
    /// See [`CalFile::occurrences_within`] for details.
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

    /// Deletes the component with given uid (including overwrites).
    ///
    /// If the containing file is empty afterwards, the file will be deleted. Otherwise, the file
    /// will just be saved.
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

    /// Saves the current state of all files to disk.
    pub fn save(&self) -> Result<(), ColError> {
        for i in &self.files {
            i.save()?;
        }
        Ok(())
    }
}
