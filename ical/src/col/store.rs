use chrono::DateTime;
use chrono_tz::Tz;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::col::{CalDir, CalFile, ColError, Occurrence};
use crate::objects::{CalCompType, CalComponent, CalDate, CalEvent, CalTodo};

#[derive(Default, Debug, Eq, PartialEq)]
pub struct CalStore {
    dirs: Vec<CalDir>,
}

impl CalStore {
    pub fn add(&mut self, dir: CalDir) {
        self.dirs.push(dir);
    }

    pub fn directory(&self, id: &Arc<String>) -> Option<&CalDir> {
        self.dirs.iter().find(|s| s.id() == id)
    }

    pub fn directory_mut(&mut self, id: &Arc<String>) -> Option<&mut CalDir> {
        self.dirs.iter_mut().find(|s| s.id() == id)
    }

    pub fn directories(&self) -> &[CalDir] {
        &self.dirs
    }

    pub fn dirs_for_type(&self, ty: CalCompType) -> impl Iterator<Item = &CalDir> {
        self.dirs
            .iter()
            .filter(move |src| match src.props().get("types") {
                Some(src_ty) => {
                    let types: Vec<CalCompType> = serde_json::from_str(src_ty).unwrap();
                    types.contains(&ty)
                }
                None => true,
            })
    }

    pub fn files(&self) -> impl Iterator<Item = &CalFile> {
        self.dirs.iter().flat_map(|c| c.files().iter())
    }

    pub fn file_by_id<S: AsRef<str>>(&self, uid: S) -> Option<&CalFile> {
        let uid_str = uid.as_ref();
        self.dirs.iter().find_map(|c| c.file_by_id(uid_str))
    }

    pub fn files_by_id_mut<S: AsRef<str>>(&mut self, uid: S) -> Option<&mut CalFile> {
        let uid_str = uid.as_ref();
        self.dirs.iter_mut().find_map(|c| c.file_by_id_mut(uid_str))
    }

    pub fn due_alarms_within(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
    ) -> impl Iterator<Item = Occurrence<'_>> {
        self.dirs
            .iter()
            .flat_map(move |c| c.due_alarms_within(start, end))
    }

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

    pub fn occurrences_within<F>(
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
            .flat_map(move |i| i.occurrences_within(start, end, filter.clone()))
    }

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

    pub fn todos(&self) -> impl Iterator<Item = &CalTodo> {
        self.files().flat_map(|i| i.todos())
    }

    pub fn events(&self) -> impl Iterator<Item = &CalEvent> {
        self.files().flat_map(|i| i.events())
    }

    pub fn switch_directory(
        &mut self,
        path: PathBuf,
        old: &Arc<String>,
        new: &Arc<String>,
    ) -> Result<(), ColError> {
        let old_src = self
            .directory_mut(old)
            .ok_or_else(|| ColError::DirNotFound((*old).to_string()))?;
        let mut file = old_src.delete_file(&path)?;

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

    pub fn save(&self) -> Result<(), ColError> {
        for s in &self.dirs {
            s.save()?;
        }
        Ok(())
    }
}
