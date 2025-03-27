use chrono::DateTime;
use chrono_tz::Tz;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::col::{AlarmOccurrence, CalDir, CalFile, ColError, Occurrence};
use crate::objects::{AlarmOverlay, CalComponent, CalDate, CalEvent, CalTodo};

/// A container for multiple [`CalDir`]s.
///
/// This container provides convenience APIs to do operations on multiple directories.
#[derive(Default, Debug, Eq, PartialEq)]
pub struct CalStore {
    dirs: Vec<CalDir>,
}

impl CalStore {
    /// Adds the given directory to the store.
    pub fn add(&mut self, dir: CalDir) {
        self.dirs.push(dir);
    }

    /// Returns a reference to the directory with given id.
    pub fn directory(&self, id: &Arc<String>) -> Option<&CalDir> {
        self.dirs.iter().find(|s| s.id() == id)
    }

    /// Returns a mutable reference to the directory with given id.
    pub fn directory_mut(&mut self, id: &Arc<String>) -> Option<&mut CalDir> {
        self.dirs.iter_mut().find(|s| s.id() == id)
    }

    /// Returns a slice of the contained directories.
    pub fn directories(&self) -> &[CalDir] {
        &self.dirs
    }

    /// Returns an iterator with all files in all directories.
    pub fn files(&self) -> impl Iterator<Item = &CalFile> {
        self.dirs.iter().flat_map(|c| c.files().iter())
    }

    /// Returns a reference to the file with given uid.
    pub fn file_by_id<S: AsRef<str>>(&self, uid: S) -> Option<&CalFile> {
        let uid_str = uid.as_ref();
        self.dirs.iter().find_map(|c| c.file_by_id(uid_str))
    }

    /// Returns a mutable reference to the file with given uid.
    pub fn files_by_id_mut<S: AsRef<str>>(&mut self, uid: S) -> Option<&mut CalFile> {
        let uid_str = uid.as_ref();
        self.dirs.iter_mut().find_map(|c| c.file_by_id_mut(uid_str))
    }

    /// Returns a vector of occurrences whose alarm is due in the given time period.
    ///
    /// Note that excluded occurrences are not returned.
    pub fn due_alarms_between<'s, 'a>(
        &'s self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
        overlay: &'a dyn AlarmOverlay,
    ) -> impl Iterator<Item = AlarmOccurrence<'s>> + use<'s, 'a> {
        self.dirs
            .iter()
            .flat_map(move |c| c.due_alarms_between(start, end, overlay))
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
        self.dirs
            .iter()
            .find_map(|c| c.occurrence_by_id(uid_str, rid, tz))
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
        self.dirs
            .iter()
            .flat_map(|c| c.files().iter())
            .flat_map(move |i| i.occurrences_between(start, end, filter.clone()))
    }

    /// Returns a [`HashMap`] with all contacts that occur in this store.
    ///
    /// The key of the hashmap is the address, whereas the value is the common name, if known, or
    /// the address otherwise. The contacts are collected by the list of attendees in all
    /// components.
    pub fn contacts(&self) -> HashMap<String, String> {
        let mut contacts = HashMap::new();
        for i in self.files() {
            let file_contacts = i.contacts();
            for (k, v) in file_contacts {
                match contacts.get_mut(&k) {
                    Some(cur_name) if *cur_name == k => {
                        *cur_name = v;
                    }
                    None => {
                        contacts.insert(k, v);
                    }
                    _ => {}
                }
            }
        }
        contacts
    }

    /// Returns an iterator with all TODOs in this store.
    pub fn todos(&self) -> impl Iterator<Item = &CalTodo> {
        self.files().flat_map(|i| i.todos())
    }

    /// Returns an iterator with all events in this store.
    pub fn events(&self) -> impl Iterator<Item = &CalEvent> {
        self.files().flat_map(|i| i.events())
    }

    /// Switches the directory of the file with given path.
    ///
    /// The `path` denotes the path of the file to delete, `old` specifies the current id of the
    /// directory that contains this file, whereas `new` specifies the id of the new directory the
    /// file should be moved to.
    ///
    /// This method assumes that both the old and the new directory exists and the file is present
    /// in the old directory. If that's not the case, an appropriate error is returned.
    ///
    /// The method will also try to be atomic: either the operation is successful or the old state
    /// is restored. This however assumes that, in some cases, the file can be moved back. If that
    /// fails, this method panics.
    pub fn switch_directory(
        &mut self,
        path: PathBuf,
        old: &Arc<String>,
        new: &Arc<String>,
    ) -> Result<(), ColError> {
        let old_src = self
            .directory_mut(old)
            .ok_or_else(|| ColError::DirNotFound((*old).to_string()))?;
        let mut file = old_src.remove_file(&path)?;

        let new_src = match self.directory_mut(new) {
            Some(src) => src,
            None => {
                // if that failed, store the file in the old directory again
                file.save().unwrap();
                self.directory_mut(old).unwrap().add_file(file);
                return Err(ColError::DirNotFound((*new).to_string()));
            }
        };

        file.set_directory(new.clone());
        file.set_path(new_src.path().join(file.path().file_name().unwrap()));
        if let Err(e) = file.save() {
            // if that failed, change everything back
            let old_src = self.directory(old).unwrap();
            file.set_directory(old.clone());
            file.set_path(old_src.path().join(file.path().file_name().unwrap()));
            file.save().unwrap();
            self.directory_mut(old).unwrap().add_file(file);
            return Err(e);
        }
        new_src.add_file(file);
        Ok(())
    }

    /// Saves all files in all directories to disk.
    pub fn save(&self) -> Result<(), ColError> {
        for s in &self.dirs {
            s.save()?;
        }
        Ok(())
    }
}
