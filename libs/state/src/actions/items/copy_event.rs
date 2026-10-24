// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{io::Write, sync::Arc};

use anyhow::{Context, bail};
use chrono::{DateTime, NaiveDateTime, Utc};
use eventix_ical::{
    col::CalFile,
    objects::{CalCompType, CalDate, CalDateTime, EventLike, UpdatableEventLike},
};
use uuid::Uuid;

use super::ItemId;
use crate::{State, actions::Action};

/// Copies a non-recurring event to a display-local time, preserving its details and span.
///
/// Expected bounds reject stale drags. The source is unchanged, and the copy receives a new UID.
#[derive(Debug)]
pub struct CopyEvent {
    pub item: ItemId,
    pub start: NaiveDateTime,
    pub expected_start: DateTime<Utc>,
    pub expected_end: DateTime<Utc>,
}

impl Action for CopyEvent {
    fn affected_items(&self) -> &[ItemId] {
        std::slice::from_ref(&self.item)
    }

    fn execute(self: Box<Self>, state: &mut State) -> anyhow::Result<ItemId> {
        let locale = state.locale();
        let tz = *state.timezone();
        let (collection, settings) = state
            .settings()
            .calendar(&self.item.calendar)
            .with_context(|| {
                locale
                    .translate("Calendar is no longer available")
                    .to_string()
            })?;
        if collection.is_read_only() || !settings.types().contains(&CalCompType::Event) {
            bail!(
                locale
                    .translate("Calendar does not allow creating this item")
                    .to_string()
            );
        }
        let user_mail = collection.email().map(|email| email.address().clone());
        let directory_id = Arc::new(self.item.calendar.clone());
        let directory = state.store_mut().directory_mut(&directory_id)?;
        let file = directory.file_by_id_mut(&self.item.uid)?;
        let occurrence = file
            .occurrence_by_id(&self.item.uid, self.item.recurrence.as_ref(), &tz)
            .with_context(|| {
                locale
                    .translate("Occurrence is no longer available")
                    .to_string()
            })?;
        if occurrence.occurrence_start().map(|date| date.to_utc()) != Some(self.expected_start)
            || occurrence.occurrence_end().map(|date| date.to_utc()) != Some(self.expected_end)
        {
            bail!(locale.translate("error.component_was_modified").to_string());
        }
        if !occurrence.is_owned_by(user_mail.as_ref())
            || occurrence.is_recurrent()
            || occurrence.is_excluded()
            || self.item.recurrence.is_some()
        {
            bail!(locale.translate("Could not apply changes").to_string());
        }

        let mut component = file
            .component_with(|component| component.uid() == &self.item.uid)
            .with_context(|| {
                locale
                    .translate("Occurrence is no longer available")
                    .to_string()
            })?
            .clone();
        let start = if component.is_all_day() {
            CalDate::Date(self.start.date(), CalCompType::Event.into())
        } else {
            CalDate::DateTime(CalDateTime::Timezone(self.start, tz.name().into()))
        };
        component
            .as_event_mut()
            .with_context(|| locale.translate("Component is not an event").to_string())?
            .shift_to(&file.calendar().date_context(), start, &tz)?;
        let uid = Uuid::new_v4().to_string();
        component.set_uid(uid.clone());
        component.set_last_modified(CalDate::now());
        component.set_stamp(CalDate::now());

        // Retain custom timezone definitions and calendar properties from the source file.
        let mut content = file.calendar().clone();
        content.delete_components(|_| true);
        content.add_component(component);
        content.populate_timezones();
        let path = directory.path().join(format!("{uid}.ics"));
        let mut output = tempfile::NamedTempFile::new_in(directory.path())?;
        content.write(output.as_file_mut())?;
        output.flush()?;
        output.as_file().sync_all()?;
        output.persist_noclobber(&path)?;
        directory.add_file(CalFile::new(directory_id, path, content))?;

        Ok(ItemId {
            calendar: self.item.calendar,
            uid,
            recurrence: None,
        })
    }
}
