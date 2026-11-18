// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use anyhow::Context;

use super::ItemId;
use crate::{State, actions::LockedAction};

/// Deletes an item, including all occurrences and overrides of a recurring series.
#[derive(Debug)]
pub struct DeleteItem {
    pub item: ItemId,
}

#[async_trait::async_trait]
impl LockedAction for DeleteItem {
    fn description(&self) -> &'static str {
        "Item deleted"
    }

    fn affected_items(&self) -> &[ItemId] {
        std::slice::from_ref(&self.item)
    }

    async fn execute_locked(self: Box<Self>, state: &mut State) -> anyhow::Result<ItemId> {
        let locale = state.locale();
        state
            .settings()
            .calendar(&self.item.calendar)
            .with_context(|| {
                locale
                    .translate("Calendar is no longer available")
                    .to_string()
            })?;

        state
            .store_mut()
            .directory_mut(&Arc::new(self.item.calendar.clone()))?
            .delete_by_uid(&self.item.uid)?;

        Ok(ItemId {
            recurrence: None,
            ..self.item
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc};

    use eventix_ical::col::CalDir;

    use super::{DeleteItem, ItemId, LockedAction};
    use crate::{CalendarSettings, CollectionSettings, State, SyncerType, actions::Action};

    fn fixture(path: &Path, read_only: bool) -> State {
        fs::write(
            path.join("event.ics"),
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\n\
             UID:delete-me\r\nDTSTART:20261012T090000Z\r\nDTEND:20261012T100000Z\r\n\
             RRULE:FREQ=DAILY;COUNT=5\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\n\
             UID:delete-me\r\nRECURRENCE-ID:20261013T090000Z\r\n\
             DTSTART:20261013T110000Z\r\nDTEND:20261013T120000Z\r\n\
             END:VEVENT\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        fs::write(
            path.join("other.ics"),
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\n\
             UID:keep-me\r\nSUMMARY:Keep me\r\nEND:VTODO\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        let xdg = Arc::new(crate::with_test_xdg(path, path));
        let mut state = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        state.store_mut().add(
            CalDir::new_from_dir(
                Arc::new("test".into()),
                path.to_path_buf(),
                "Test".into(),
                read_only,
            )
            .unwrap(),
        );
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: path.display().to_string(),
        });
        let mut settings = CalendarSettings::default();
        settings.set_enabled(true);
        collection
            .all_calendars_mut()
            .insert("test".into(), settings);
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        state
    }

    fn command(uid: &str) -> Box<DeleteItem> {
        Box::new(DeleteItem {
            item: ItemId {
                calendar: "test".into(),
                uid: uid.into(),
                recurrence: None,
            },
        })
    }

    #[tokio::test]
    async fn deletion_removes_series_and_overrides_and_preserves_other_items() {
        let tmp = tempfile::tempdir().unwrap();
        let state = Arc::new(tokio::sync::Mutex::new(fixture(tmp.path(), false)));
        let other = fs::read(tmp.path().join("other.ics")).unwrap();
        let action: Box<dyn Action> = command("delete-me");
        let item = action.execute(&state).await.unwrap();
        assert_eq!(item, command("delete-me").item);
        assert!(!tmp.path().join("event.ics").exists());
        assert!(
            state
                .lock()
                .await
                .store_mut()
                .file_by_id_mut("delete-me")
                .is_err()
        );
        assert_eq!(fs::read(tmp.path().join("other.ics")).unwrap(), other);
        assert!(
            state
                .lock()
                .await
                .store_mut()
                .file_by_id_mut("keep-me")
                .is_ok()
        );

        command("keep-me").execute(&state).await.unwrap();
        assert!(!tmp.path().join("other.ics").exists());
    }

    #[tokio::test]
    async fn read_only_and_missing_items_are_rejected_without_changing_files() {
        for read_only in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let mut state = fixture(tmp.path(), read_only);
            let before = fs::read(tmp.path().join("event.ics")).unwrap();
            let uid = if read_only { "delete-me" } else { "missing" };
            assert!(command(uid).execute_locked(&mut state).await.is_err());
            assert_eq!(fs::read(tmp.path().join("event.ics")).unwrap(), before);
            assert!(state.store().file_by_id("delete-me").is_some());
        }
    }
}
