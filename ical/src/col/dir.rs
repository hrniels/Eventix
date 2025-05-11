use chrono::{DateTime, NaiveDateTime, Utc};
use chrono_tz::Tz;
use std::fmt::Display;
use std::fs::{self, read_dir};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::info;

use crate::col::{AlarmOccurrence, CalFile, ColError, Occurrence};
use crate::objects::{AlarmOverlay, CalComponent, CalDate};

/// A directory with calendar files.
///
/// A [`CalDir`] is a container for [`CalFile`] objects that are stored in a specific directory.
/// Additionally, each directory has a unique id and a human-readable name.
#[derive(Default, Debug)]
pub struct CalDir {
    id: Arc<String>,
    path: PathBuf,
    name: String,
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
    /// [`Calendar`]. These are added to the created [`CalDir`]. Note that it only considers files
    /// ending in `.ics`.
    pub fn new_from_dir(id: Arc<String>, path: PathBuf, name: String) -> Result<Self, ColError> {
        let mut files = Vec::new();
        Self::with_files(&path, |filename| {
            files.push(CalFile::new_from_file(id.clone(), filename)?);
            Ok(())
        })?;

        info!(
            "{}: found {} calendar file(s) in directory {:?}",
            id,
            files.len(),
            path
        );

        Ok(Self {
            id,
            path,
            name,
            files,
        })
    }

    /// Rescans the directory for added files.
    ///
    /// These files are added to the collection. The method returns `true` if new files were found
    /// and `false` otherwise.
    pub fn rescan_for_additions(&mut self) -> Result<bool, ColError> {
        let mut seen_changes = false;
        Self::with_files(&self.path, |filename| {
            if !self.files.iter().any(|f| f.path() == &filename) {
                info!("{}: added file {:?} during rescan", self.id, filename);
                self.files
                    .push(CalFile::new_from_file(self.id.clone(), filename)?);
                seen_changes = true;
            }
            Ok(())
        })
        .map(|_| seen_changes)
    }

    /// Rescans the directory for changes.
    ///
    /// If a file's last modification time is newer than `last_check` the contained calendar will
    /// be reloaded from file to update the collection. The method returns `true` if changed files
    /// were found and `false` otherwise.
    pub fn rescan_for_updates(&mut self, last_check: NaiveDateTime) -> Result<bool, ColError> {
        let mut seen_changes = false;
        Self::with_files(&self.path, |filename| {
            let metadata =
                fs::metadata(&filename).map_err(|_| ColError::FileMetadata(filename.clone()))?;
            let last_mod = metadata
                .modified()
                .map_err(|_| ColError::FileModified(filename.clone()))?;
            let last_mod: DateTime<Utc> = last_mod.into();
            let last_mod = last_mod.naive_utc();
            if last_mod > last_check {
                info!("{}: changed file {:?} during rescan", self.id, filename);
                let file = self
                    .files
                    .iter_mut()
                    .find(|f| f.path() == &filename)
                    .ok_or_else(|| ColError::FileNotFound(filename.clone()))?;
                seen_changes = true;
                file.reload_calendar()
            } else {
                Ok(())
            }
        })
        .map(|_| seen_changes)
    }

    /// Rescans the directory for deleted files.
    ///
    /// These files are deleted from the collection. The method returns `true` if deleted files
    /// were found and `false` otherwise.
    pub fn rescan_for_deletions(&mut self) -> bool {
        // collect all files
        let mut files = Vec::new();
        Self::with_files(&self.path, |filename| {
            files.push(filename);
            Ok(())
        })
        .unwrap();

        // now remove all objects that do no longer exists in the filesystem
        let old_len = self.files.len();
        self.files.retain(|f| {
            let exists = files.contains(f.path());
            if !exists {
                info!("{}: deleted file {:?} during rescan", self.id, f.path());
            }
            exists
        });
        self.files.len() != old_len
    }

    fn with_files<F>(path: &Path, mut func: F) -> Result<(), ColError>
    where
        F: FnMut(PathBuf) -> Result<(), ColError>,
    {
        let dir_files = read_dir(path).map_err(|e| ColError::ReadDir(path.to_path_buf(), e))?;
        for entry in dir_files {
            let entry = entry.map_err(|e| ColError::ReadDir(path.to_path_buf(), e))?;
            if !entry
                .file_type()
                .map_err(|e| ColError::FileType(path.to_path_buf(), e))?
                .is_file()
            {
                continue;
            }

            let filename = entry.path();
            if filename
                .extension()
                .and_then(|ex| ex.to_str())
                .is_none_or(|ex| ex != "ics")
            {
                continue;
            }

            func(filename)?;
        }
        Ok(())
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

    /// Returns a vector of occurrences whose alarm is due in the given time period.
    ///
    /// Note that excluded occurrences are not returned.
    pub fn due_alarms_between<'d, 'a>(
        &'d self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
        overlay: &'a dyn AlarmOverlay,
    ) -> impl Iterator<Item = AlarmOccurrence<'d>> + use<'d, 'a> {
        self.files
            .iter()
            .flat_map(move |i| i.due_alarms_between(start, end, overlay))
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
    /// See [`CalFile::occurrences_between`] for details.
    pub fn occurrences_between<F>(
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
            .flat_map(move |i| i.occurrences_between(start, end, filter.clone()))
    }

    /// Deletes the component with given uid (including overwrites).
    ///
    /// If the containing file is empty afterwards, the file will be deleted. Otherwise, the file
    /// will just be saved.
    pub fn delete_by_uid<S: AsRef<str> + ToString>(&mut self, uid: S) -> Result<(), ColError> {
        let file = self
            .file_by_id_mut(&uid)
            .ok_or_else(|| ColError::ComponentNotFound(uid.to_string()))?;
        file.delete_by_uid(uid);
        if file.components().is_empty() {
            let path = file.path().clone();
            self.remove_file(&path).map(|_| ())
        } else {
            file.save()
        }
    }

    /// Removes the [`CalFile`] from the collection that contains given uid.
    pub fn remove_by_uid<S: AsRef<str> + ToString>(&mut self, uid: S) -> Result<CalFile, ColError> {
        let idx = self
            .files
            .iter()
            .position(|i| i.contains_uid(uid.as_ref()))
            .ok_or_else(|| ColError::ComponentNotFound(uid.to_string()))?;
        Ok(self.files.remove(idx))
    }

    pub(crate) fn remove_file(&mut self, path: &PathBuf) -> Result<CalFile, ColError> {
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
