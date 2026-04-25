// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::DateTime;
use chrono_tz::Tz;
use std::collections::{HashMap, HashSet};
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
    write_protected: HashSet<Arc<String>>,
}

impl CalStore {
    /// Adds the given directory to the store.
    pub fn add(&mut self, dir: CalDir) {
        self.dirs.push(dir);
    }

    /// Only retains the directories that return true for given function
    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&CalDir) -> bool,
    {
        self.dirs.retain(f);
    }

    /// Returns a reference to the directory with given id.
    pub fn directory(&self, id: &Arc<String>) -> Option<&CalDir> {
        self.dirs.iter().find(|s| s.id() == id)
    }

    /// Returns a mutable reference to the directory with given id.
    pub fn try_directory_mut(&mut self, id: &Arc<String>) -> Result<&mut CalDir, ColError> {
        let write_protected = self.directory_write_protected(id);
        let dir = self
            .dirs
            .iter_mut()
            .find(|s| s.id() == id)
            .ok_or_else(|| ColError::DirNotFound((**id).clone()))?;
        if write_protected {
            return Err(ColError::DirWriteProtected((**id).clone()));
        }
        Ok(dir)
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
    pub fn try_file_by_id_mut<S: AsRef<str>>(&mut self, uid: S) -> Result<&mut CalFile, ColError> {
        let uid_str = uid.as_ref();
        let dir = self
            .dirs
            .iter()
            .find(|c| c.file_by_id(uid_str).is_some())
            .ok_or_else(|| ColError::ComponentNotFound(uid_str.to_string()))?
            .id()
            .clone();
        self.try_directory_mut(&dir)?
            .file_by_id_mut(uid_str)
            .ok_or_else(|| ColError::ComponentNotFound(uid_str.to_string()))
    }

    /// Returns whether the directory with the given id is write-protected.
    pub fn directory_write_protected(&self, id: &Arc<String>) -> bool {
        self.write_protected.contains(id)
    }

    /// Write-protects the given directories.
    ///
    /// Returns an error without modifying any protection state if one of the directories is already
    /// write-protected.
    pub fn protect_directories<I>(&mut self, ids: I) -> Result<(), ColError>
    where
        I: IntoIterator<Item = Arc<String>>,
    {
        let ids = ids.into_iter().collect::<Vec<_>>();
        if let Some(id) = ids.iter().find(|id| self.directory_write_protected(id)) {
            return Err(ColError::DirWriteProtected((**id).clone()));
        }
        self.write_protected.extend(ids);
        Ok(())
    }

    /// Removes write protection from the given directories.
    pub fn unprotect_directories<I>(&mut self, ids: I)
    where
        I: IntoIterator<Item = Arc<String>>,
    {
        for id in ids {
            self.write_protected.remove(&id);
        }
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
        if self.directory_write_protected(old) {
            return Err(ColError::DirWriteProtected((**old).clone()));
        }
        if self.directory_write_protected(new) {
            return Err(ColError::DirWriteProtected((**new).clone()));
        }

        let old_src = self.try_directory_mut(old)?;
        let mut file = old_src.remove_file(&path)?;

        let new_src = match self.try_directory_mut(new) {
            Ok(src) => src,
            Err(err) => {
                // if that failed, store the file in the old directory again
                file.save().unwrap();
                self.try_directory_mut(old).unwrap().add_file(file);
                return Err(err);
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
            self.try_directory_mut(old).unwrap().add_file(file);
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::col::{CalDir, CalFile, ColError};
    use crate::objects::{
        CalAttendee, CalComponent, CalEvent, CalTodo, Calendar, UpdatableEventLike,
    };

    use super::CalStore;

    // --- helpers ---

    fn make_id(s: &str) -> Arc<String> {
        Arc::new(s.to_string())
    }

    /// Builds an empty in-memory [`CalDir`] with the given id.
    fn make_dir(id: &str) -> CalDir {
        CalDir::new_empty(make_id(id), PathBuf::default(), id.to_string())
    }

    /// Builds an in-memory [`CalFile`] containing a single event with the given UID.
    fn make_event_file(uid: &str) -> CalFile {
        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(CalEvent::new(uid)));
        CalFile::new(Arc::default(), PathBuf::default(), cal)
    }

    /// Builds an in-memory [`CalFile`] containing a single TODO with the given UID.
    fn make_todo_file(uid: &str) -> CalFile {
        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Todo(CalTodo::new(uid)));
        CalFile::new(Arc::default(), PathBuf::default(), cal)
    }

    // --- add / directories ---

    #[test]
    fn add_and_directories() {
        let mut store = CalStore::default();
        assert!(store.directories().is_empty());

        store.add(make_dir("a"));
        store.add(make_dir("b"));

        assert_eq!(store.directories().len(), 2);
        assert_eq!(store.directories()[0].name(), "a");
        assert_eq!(store.directories()[1].name(), "b");
    }

    // --- retain ---

    #[test]
    fn retain_keeps_matching_dirs() {
        let mut store = CalStore::default();
        store.add(make_dir("keep"));
        store.add(make_dir("drop"));

        store.retain(|d| d.name() == "keep");

        assert_eq!(store.directories().len(), 1);
        assert_eq!(store.directories()[0].name(), "keep");
    }

    // --- directory / try_directory_mut ---

    #[test]
    fn directory_found_and_not_found() {
        let id_a = make_id("a");
        let id_b = make_id("b");
        let id_missing = make_id("missing");

        let mut store = CalStore::default();
        store.add(CalDir::new_empty(
            id_a.clone(),
            PathBuf::default(),
            "A".into(),
        ));
        store.add(CalDir::new_empty(
            id_b.clone(),
            PathBuf::default(),
            "B".into(),
        ));

        assert!(store.directory(&id_a).is_some());
        assert!(store.directory(&id_b).is_some());
        assert!(store.directory(&id_missing).is_none());

        assert!(store.try_directory_mut(&id_a).is_ok());
        assert!(matches!(
            store.try_directory_mut(&id_missing),
            Err(ColError::DirNotFound(_))
        ));
    }

    // --- files ---

    #[test]
    fn files_iterator_over_multiple_dirs() {
        let mut store = CalStore::default();

        let mut dir_a = make_dir("a");
        dir_a.add_file(make_event_file("uid-1"));
        dir_a.add_file(make_event_file("uid-2"));
        store.add(dir_a);

        let mut dir_b = make_dir("b");
        dir_b.add_file(make_event_file("uid-3"));
        store.add(dir_b);

        let all_files: Vec<_> = store.files().collect();
        assert_eq!(all_files.len(), 3);
    }

    // --- file_by_id / try_file_by_id_mut ---

    #[test]
    fn file_by_id_found_and_not_found() {
        let mut store = CalStore::default();

        let mut dir_a = make_dir("a");
        dir_a.add_file(make_event_file("uid-in-a"));
        store.add(dir_a);

        let mut dir_b = make_dir("b");
        dir_b.add_file(make_event_file("uid-in-b"));
        store.add(dir_b);

        // file_by_id searches across all dirs
        assert!(store.file_by_id("uid-in-a").is_some());
        assert!(store.file_by_id("uid-in-b").is_some());
        assert!(store.file_by_id("uid-absent").is_none());

        // try_file_by_id_mut variant
        assert!(store.try_file_by_id_mut("uid-in-a").is_ok());
        assert!(matches!(
            store.try_file_by_id_mut("uid-absent"),
            Err(ColError::ComponentNotFound(_))
        ));
    }

    #[test]
    fn try_directory_mut_fails_for_write_protected_directory() {
        let mut store = CalStore::default();
        let id = make_id("protected");
        store.add(CalDir::new_empty(
            id.clone(),
            PathBuf::default(),
            "Protected".into(),
        ));

        store.protect_directories(vec![id.clone()]).unwrap();

        assert!(matches!(
            store.try_directory_mut(&id),
            Err(ColError::DirWriteProtected(ref protected_id)) if protected_id == &*id
        ));
    }

    #[test]
    fn try_file_by_id_mut_fails_for_write_protected_directory() {
        let mut store = CalStore::default();
        let id = make_id("protected");
        let mut dir = CalDir::new_empty(id.clone(), PathBuf::default(), "Protected".into());
        dir.add_file(make_event_file("uid-1"));
        store.add(dir);

        store.protect_directories(vec![id.clone()]).unwrap();

        assert!(matches!(
            store.try_file_by_id_mut("uid-1"),
            Err(ColError::DirWriteProtected(ref protected_id)) if protected_id == &*id
        ));
    }

    // --- todos / events ---

    #[test]
    fn todos_and_events_iterators() {
        let mut store = CalStore::default();

        let mut dir = make_dir("mixed");
        dir.add_file(make_event_file("ev-1"));
        dir.add_file(make_event_file("ev-2"));
        dir.add_file(make_todo_file("td-1"));
        store.add(dir);

        assert_eq!(store.events().count(), 2);
        assert_eq!(store.todos().count(), 1);
    }

    // --- contacts ---

    #[test]
    fn contacts_empty_store() {
        let store = CalStore::default();
        assert!(store.contacts().is_empty());
    }

    #[test]
    fn contacts_deduplication_and_upgrade() {
        // Dir A: address without a CN (will be inserted as key == value).
        let mut cal_a = Calendar::default();
        let mut ev_a = CalEvent::new("ev-a");
        ev_a.set_attendees(Some(vec![CalAttendee::new(
            "alice@example.com".to_string(),
        )]));
        cal_a.add_component(CalComponent::Event(ev_a));
        let file_a = CalFile::new(Arc::default(), PathBuf::default(), cal_a);
        let mut dir_a = make_dir("a");
        dir_a.add_file(file_a);

        // Dir B: same address, but this time with a CN. The `contacts()` method must upgrade
        // the existing entry from the bare address to the human-readable name.
        let mut cal_b = Calendar::default();
        let mut ev_b = CalEvent::new("ev-b");
        let mut att = CalAttendee::new("alice@example.com".to_string());
        att.set_common_name("Alice Wonderland".to_string());
        ev_b.set_attendees(Some(vec![att]));
        cal_b.add_component(CalComponent::Event(ev_b));
        let file_b = CalFile::new(Arc::default(), PathBuf::default(), cal_b);
        let mut dir_b = make_dir("b");
        dir_b.add_file(file_b);

        let mut store = CalStore::default();
        store.add(dir_a);
        store.add(dir_b);

        let contacts = store.contacts();
        // The address should be present and its display name upgraded to the CN.
        assert_eq!(
            contacts.get("alice@example.com").map(String::as_str),
            Some("Alice Wonderland")
        );
    }

    #[test]
    fn contacts_already_named_not_downgraded() {
        // If an address is first seen with a CN it must not be replaced by a bare address
        // from a subsequent file (the `_ => {}` branch).
        let mut cal_a = Calendar::default();
        let mut ev_a = CalEvent::new("ev-a");
        let mut att_a = CalAttendee::new("bob@example.com".to_string());
        att_a.set_common_name("Bob Named".to_string());
        ev_a.set_attendees(Some(vec![att_a]));
        cal_a.add_component(CalComponent::Event(ev_a));
        let file_a = CalFile::new(Arc::default(), PathBuf::default(), cal_a);
        let mut dir_a = make_dir("a");
        dir_a.add_file(file_a);

        // Second file with the same address but no CN.
        let mut cal_b = Calendar::default();
        let mut ev_b = CalEvent::new("ev-b");
        ev_b.set_attendees(Some(vec![CalAttendee::new("bob@example.com".to_string())]));
        cal_b.add_component(CalComponent::Event(ev_b));
        let file_b = CalFile::new(Arc::default(), PathBuf::default(), cal_b);
        let mut dir_b = make_dir("b");
        dir_b.add_file(file_b);

        let mut store = CalStore::default();
        store.add(dir_a);
        store.add(dir_b);

        let contacts = store.contacts();
        // The CN from the first encounter must be preserved.
        assert_eq!(
            contacts.get("bob@example.com").map(String::as_str),
            Some("Bob Named")
        );
    }
}
