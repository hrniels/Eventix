// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::DateTime;
use chrono_tz::Tz;
use std::fmt::Display;
use std::fs::read_dir;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use crate::col::{AlarmOccurrence, CalFile, ColError, Occurrence};
use crate::objects::{AlarmOverlay, CalComponent, CalDate, Calendar, EventLike};

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
    read_only: bool,
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
    /// Creates a new empty directory with the given path.
    pub fn new_empty(id: Arc<String>, path: PathBuf, name: String, read_only: bool) -> Self {
        Self {
            id,
            path,
            name,
            files: vec![],
            read_only,
        }
    }

    /// Creates a new directory from the given path.
    ///
    /// This method reads all files in the given directory and tries to parse them into a
    /// [`Calendar`]. These are added to the created [`CalDir`]. Note that it only considers files
    /// ending in `.ics`.
    pub fn new_from_dir(
        id: Arc<String>,
        path: PathBuf,
        name: String,
        read_only: bool,
    ) -> Result<Self, ColError> {
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
            read_only,
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

    /// Rescans the directory and reloads all files.
    pub fn rescan_files(&mut self) -> Result<bool, ColError> {
        let mut seen_changes = false;
        Self::with_files(&self.path, |filename| {
            info!("{}: changed file {:?} during rescan", self.id, filename);
            let file = self
                .files
                .iter_mut()
                .find(|f| f.path() == &filename)
                .ok_or_else(|| ColError::FileNotFound(filename.clone()))?;
            seen_changes = true;
            file.reload_calendar()
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

    /// Sets the human-readable name to given one.
    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    /// Returns a slice with all files in this directory.
    pub fn files(&self) -> &[CalFile] {
        &self.files
    }

    /// Returns a mutable slice with all files in this directory.
    ///
    /// This call fails if this directory is read only.
    pub fn files_mut(&mut self) -> Result<&mut [CalFile], ColError> {
        if self.read_only {
            return Err(ColError::DirWriteProtected((*self.id).clone()));
        }
        Ok(&mut self.files)
    }

    /// Returns a reference to the file that hosts the component with given uid.
    pub fn file_by_id<S: AsRef<str>>(&self, uid: S) -> Option<&CalFile> {
        let uid_ref = uid.as_ref();
        self.files.iter().find(|i| i.contains_uid(uid_ref))
    }

    /// Returns a mutable reference to the file that hosts the component with given uid.
    ///
    /// This call fails if this directory is read only.
    pub fn file_by_id_mut<S: AsRef<str>>(&mut self, uid: S) -> Result<&mut CalFile, ColError> {
        if self.read_only {
            return Err(ColError::DirWriteProtected((*self.id).clone()));
        }
        let uid_ref = uid.as_ref();
        self.files
            .iter_mut()
            .find(|i| i.contains_uid(uid_ref))
            .ok_or_else(|| ColError::ComponentNotFound(uid_ref.to_string()))
    }

    /// Adds the given file to this directory.
    pub fn add_file(&mut self, file: CalFile) -> Result<(), ColError> {
        if self.read_only {
            return Err(ColError::DirWriteProtected((*self.id).clone()));
        }
        self.files.push(file);
        Ok(())
    }

    /// Imports a parsed external calendar, saving one resource per UID.
    ///
    /// Replaces existing UIDs at their current paths and assigns generated filenames to new ones.
    /// Recurrence overrides and calendar metadata are retained when splitting resources.
    /// Returns an error if the directory is read-only or a resource cannot be saved. Resources are
    /// processed in UID order; earlier successful writes remain on disk and in memory on failure.
    pub fn import(&mut self, calendar: Calendar) -> Result<(), ColError> {
        if self.read_only {
            return Err(ColError::DirWriteProtected((*self.id).clone()));
        }
        let mut calendars = calendar.split_by_uid();
        calendars.sort_by(|a, b| a.components()[0].uid().cmp(b.components()[0].uid()));
        for calendar in calendars {
            let uid = calendar.components()[0].uid();
            let index = self.files.iter().position(|file| file.contains_uid(uid));
            let path = index
                .map(|index| self.files[index].path().clone())
                .unwrap_or_else(|| self.path.join(format!("{}.ics", Uuid::new_v4())));
            let file = CalFile::new(self.id.clone(), path, calendar);
            file.save()?;
            match index {
                Some(index) => self.files[index] = file,
                None => self.files.push(file),
            }
        }
        Ok(())
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
    /// will just be saved. This call fails if this directory is read only.
    pub fn delete_by_uid<S: AsRef<str> + ToString>(&mut self, uid: S) -> Result<(), ColError> {
        if self.read_only {
            return Err(ColError::DirWriteProtected((*self.id).clone()));
        }
        let file = self.file_by_id_mut(&uid)?;
        file.delete_by_uid(uid);
        if file.components().is_empty() {
            let path = file.path().clone();
            self.remove_file(&path).map(|_| ())
        } else {
            file.save()
        }
    }

    /// Removes the [`CalFile`] from the collection that contains given uid.
    ///
    /// This call fails if this directory is read only.
    pub fn remove_by_uid<S: AsRef<str> + ToString>(&mut self, uid: S) -> Result<CalFile, ColError> {
        if self.read_only {
            return Err(ColError::DirWriteProtected((*self.id).clone()));
        }
        let idx = self
            .files
            .iter()
            .position(|i| i.contains_uid(uid.as_ref()))
            .ok_or_else(|| ColError::ComponentNotFound(uid.to_string()))?;
        Ok(self.files.remove(idx))
    }

    pub(crate) fn remove_file(&mut self, path: &PathBuf) -> Result<CalFile, ColError> {
        if self.read_only {
            return Err(ColError::DirWriteProtected((*self.id).clone()));
        }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::col::CalFile;
    use crate::objects::{CalComponent, CalEvent, Calendar, EventLike, UpdatableEventLike};

    use super::CalDir;

    /// Builds a minimal in-memory [`CalFile`] that contains a single event with the given UID.
    fn make_file(uid: &str) -> CalFile {
        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(CalEvent::new(uid)));
        CalFile::new(Arc::default(), PathBuf::default(), cal)
    }

    fn import_calendar(uid: &str, summary: &str) -> Calendar {
        let mut event = CalEvent::new(uid);
        event.set_summary(Some(summary.into()));
        let mut calendar = Calendar::default();
        calendar.add_component(CalComponent::Event(event));
        calendar
    }

    #[test]
    fn import_generates_safe_filenames_and_replaces_existing_resources_at_the_same_path() {
        let tmp = tempfile::tempdir().unwrap();
        let id = Arc::new(String::from("test"));
        let mut dir = CalDir::new_empty(id, tmp.path().into(), "Test".into(), false);
        // Long UIDs and path syntax must not become part of the filesystem path.
        let uid = format!("../../{}", "ü".repeat(300));
        dir.import(import_calendar(&uid, "Original")).unwrap();
        let path = dir.file_by_id(&uid).unwrap().path().clone();
        assert_eq!(path.parent(), Some(tmp.path()));
        assert_eq!(path.extension().unwrap(), "ics");
        assert!(path.file_name().unwrap().len() < 100);
        dir.import(import_calendar(&uid, "Replacement")).unwrap();
        assert_eq!(dir.files().len(), 1);
        assert_eq!(dir.file_by_id(&uid).unwrap().path(), &path);
        assert_eq!(
            dir.file_by_id(&uid).unwrap().components()[0]
                .summary()
                .map(String::as_str),
            Some("Replacement")
        );
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("Replacement")
        );
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[test]
    fn import_reuses_arbitrary_existing_filenames_without_deleting_them_first() {
        let tmp = tempfile::tempdir().unwrap();
        let id = Arc::new(String::from("test"));
        let path = tmp.path().join("remote-resource.ics");
        let original = CalFile::new(id.clone(), path.clone(), import_calendar("uid", "Original"));
        original.save().unwrap();
        let mut dir = CalDir::new_from_dir(id, tmp.path().into(), "Test".into(), false).unwrap();
        dir.import(import_calendar("uid", "Updated")).unwrap();
        assert_eq!(dir.files().len(), 1);
        assert_eq!(dir.file_by_id("uid").unwrap().path(), &path);
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[test]
    fn import_read_only_directory_rejects_changes_before_saving() {
        let tmp = tempfile::tempdir().unwrap();
        let mut dir = CalDir::new_empty(
            Arc::new("test".into()),
            tmp.path().into(),
            "Test".into(),
            true,
        );
        assert!(matches!(
            dir.import(import_calendar("new", "New")),
            Err(super::ColError::DirWriteProtected(_))
        ));
        assert!(dir.files().is_empty());
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }

    #[test]
    fn import_failure_keeps_completed_resources_and_does_not_publish_the_failed_replacement() {
        let tmp = tempfile::tempdir().unwrap();
        let id = Arc::new(String::from("test"));
        let mut dir = CalDir::new_empty(id.clone(), tmp.path().into(), "Test".into(), false);
        dir.import(import_calendar("a", "Original A")).unwrap();
        let blocked = tmp.path().join("blocked.ics");
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(blocked.join("keep"), "untouched").unwrap();
        dir.add_file(CalFile::new(
            id,
            blocked.clone(),
            import_calendar("b", "Original B"),
        ))
        .unwrap();
        let mut input = import_calendar("a", "Updated A");
        input.add_component(import_calendar("b", "Updated B").components()[0].clone());
        input.add_component(import_calendar("c", "New C").components()[0].clone());
        assert!(dir.import(input).is_err());
        let a = dir.file_by_id("a").unwrap();
        assert_eq!(
            a.components()[0].summary().map(String::as_str),
            Some("Updated A")
        );
        assert!(
            std::fs::read_to_string(a.path())
                .unwrap()
                .contains("Updated A")
        );
        assert_eq!(
            dir.file_by_id("b").unwrap().components()[0]
                .summary()
                .map(String::as_str),
            Some("Original B")
        );
        assert_eq!(
            std::fs::read_to_string(blocked.join("keep")).unwrap(),
            "untouched"
        );
        assert!(dir.file_by_id("c").is_none());
        // No temporary files are left after the failed save.
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 2);
    }

    #[test]
    fn import_splits_uids_and_preserves_recurrence_overrides_and_timezones() {
        let tmp = tempfile::tempdir().unwrap();
        let mut dir = CalDir::new_empty(
            Arc::new("test".into()),
            tmp.path().into(),
            "Test".into(),
            false,
        );
        let input: Calendar = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
            BEGIN:VTIMEZONE\r\nTZID:Custom\r\nBEGIN:STANDARD\r\n\
            DTSTART:19700101T000000\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0200\r\n\
            END:STANDARD\r\nEND:VTIMEZONE\r\n\
            BEGIN:VEVENT\r\nUID:series\r\nDTSTART;TZID=Custom:20261109T090000\r\n\
            RRULE:FREQ=DAILY;COUNT=3\r\nSUMMARY:Series\r\nEND:VEVENT\r\n\
            BEGIN:VEVENT\r\nUID:series\r\nRECURRENCE-ID;TZID=Custom:20261110T090000\r\n\
            DTSTART;TZID=Custom:20261110T100000\r\nSUMMARY:Exception\r\nEND:VEVENT\r\n\
            BEGIN:VTODO\r\nUID:task\r\nSUMMARY:Task\r\nEND:VTODO\r\n\
            END:VCALENDAR\r\n"
            .parse()
            .unwrap();
        dir.import(input).unwrap();
        assert_eq!(dir.files().len(), 2);
        let series = dir.file_by_id("series").unwrap();
        assert_eq!(series.components().len(), 2);
        let serialized = std::fs::read_to_string(series.path()).unwrap();
        assert!(serialized.contains("TZID:Custom"));
        assert!(serialized.contains("RECURRENCE-ID;TZID=Custom:"));
        assert!(!serialized.contains("UID:task"));
        assert!(dir.file_by_id("task").is_some());
    }

    #[test]
    fn display_shows_name() {
        let dir = CalDir::new_empty(
            Arc::new("id1".into()),
            PathBuf::from("/tmp"),
            "My Cal".into(),
            false,
        );
        assert_eq!(format!("{dir}"), "My Cal");
    }

    #[test]
    fn partial_eq_same_name_and_files() {
        // Two dirs with the same name and no files are equal regardless of id/path.
        let a = CalDir::new_empty(
            Arc::new("id".into()),
            PathBuf::from("/a"),
            "Work".into(),
            false,
        );
        let b = CalDir::new_empty(
            Arc::new("id".into()),
            PathBuf::from("/b"),
            "Work".into(),
            false,
        );
        assert_eq!(a, b);

        // Different name makes them unequal even with no files.
        let c = CalDir::new_empty(
            Arc::new("id".into()),
            PathBuf::from("/c"),
            "Personal".into(),
            false,
        );
        assert_ne!(a, c);
    }

    #[test]
    fn new_empty_creates_empty_dir() {
        let id: Arc<String> = Arc::new("cal-id".into());
        let path = PathBuf::from("/some/path");
        let dir = CalDir::new_empty(id.clone(), path.clone(), "Test".into(), false);

        assert_eq!(dir.id(), &id);
        assert_eq!(dir.path(), &path);
        assert_eq!(dir.name(), "Test");
        assert!(dir.files().is_empty());
    }

    #[test]
    fn set_name_updates_name() {
        let mut dir = CalDir::new_empty(
            Arc::new("id".into()),
            PathBuf::from("/tmp"),
            "Old".into(),
            false,
        );
        dir.set_name("New".into());
        assert_eq!(dir.name(), "New");
    }

    #[test]
    fn files_mut_allows_mutation() {
        let mut dir = CalDir::new_empty(
            Arc::new("id".into()),
            PathBuf::from("/tmp"),
            "Old".into(),
            false,
        );
        dir.add_file(make_file("ev-x")).unwrap();
        // files_mut must expose the same slice length.
        assert_eq!(dir.files_mut().unwrap().len(), 1);
    }

    #[test]
    fn file_by_id_found_and_not_found() {
        let mut dir = CalDir::new_empty(
            Arc::new("id".into()),
            PathBuf::from("/tmp"),
            "Old".into(),
            false,
        );
        dir.add_file(make_file("uid-alpha")).unwrap();
        dir.add_file(make_file("uid-beta")).unwrap();

        assert!(dir.file_by_id("uid-alpha").is_some());
        assert!(dir.file_by_id("uid-beta").is_some());
        assert!(dir.file_by_id("uid-missing").is_none());

        assert!(dir.file_by_id_mut("uid-alpha").is_ok());
        assert!(dir.file_by_id_mut("uid-nope").is_err());
    }

    #[test]
    fn remove_by_uid_success_and_error() {
        let mut dir = CalDir::new_empty(
            Arc::new("id".into()),
            PathBuf::from("/tmp"),
            "Old".into(),
            false,
        );
        dir.add_file(make_file("uid-rm")).unwrap();
        dir.add_file(make_file("uid-keep")).unwrap();

        // Successful removal: returns the file and shrinks the collection.
        let removed = dir.remove_by_uid("uid-rm");
        assert!(removed.is_ok());
        assert_eq!(dir.files().len(), 1);
        assert!(dir.file_by_id("uid-rm").is_none());
        assert!(dir.file_by_id("uid-keep").is_some());

        // Removing a UID that no longer exists returns an error.
        assert!(dir.remove_by_uid("uid-rm").is_err());
    }
}
