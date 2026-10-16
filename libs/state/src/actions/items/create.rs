// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{io::Write, sync::Arc};

use anyhow::{Context, bail};
use eventix_ical::{
    col::CalFile,
    objects::{CalCompType, CalComponent, CalEvent, CalOrganizer, CalTodo, Calendar, DateContext},
};
use eventix_locale::Locale;
use uuid::Uuid;

use super::ItemId;
use crate::{CalendarAlarmType, PersonalAlarms, State};

/// Creates and persists an item in a writable calendar supporting its type.
///
/// The callback populates a new component before it is saved. The returned identity can be
/// used to refresh queries. Successful creation remembers the calendar for the next item.
pub fn create_component<F>(
    state: &mut State,
    locale: &Arc<dyn Locale + Send + Sync>,
    calendar: &str,
    ctype: CalCompType,
    populate: F,
) -> anyhow::Result<ItemId>
where
    F: FnOnce(
        &String,
        &CalendarAlarmType,
        &mut CalComponent,
        &mut PersonalAlarms,
        Option<CalOrganizer>,
        &DateContext,
        &Arc<dyn Locale + Send + Sync>,
    ) -> anyhow::Result<()>,
{
    let calendar = Arc::new(calendar.to_string());
    let (collection, settings) = state.settings().calendar(&calendar).with_context(|| {
        locale
            .translate("Calendar is no longer available")
            .to_string()
    })?;

    if collection.is_read_only() || !settings.types().contains(&ctype) {
        bail!(
            locale
                .translate("Calendar does not allow creating this item")
                .to_string()
        );
    }

    let organizer = collection.build_organizer();
    let alarm_type = settings.alarms().clone();

    // Check the directory before invoking callbacks with side effects such as personal alarms.
    state.store_mut().directory_mut(&calendar)?.files_mut()?;
    let uid = Uuid::new_v4().to_string();
    let mut component = match ctype {
        CalCompType::Event => CalComponent::Event(CalEvent::new(&uid)),
        CalCompType::Todo => CalComponent::Todo(CalTodo::new(&uid)),
    };

    populate(
        &calendar,
        &alarm_type,
        &mut component,
        state.personal_alarms_mut(),
        organizer,
        &DateContext::system(),
        locale,
    )?;

    let directory = state.store_mut().directory_mut(&calendar)?;
    let path = directory.path().join(format!("{uid}.ics"));
    let mut content = Calendar::default();
    content.add_component(component);
    content.populate_timezones();

    let mut output = tempfile::NamedTempFile::new_in(directory.path())?;
    content.write(output.as_file_mut())?;
    output.flush()?;
    output.as_file().sync_all()?;
    output.persist_noclobber(&path)?;

    let file = CalFile::new(calendar.clone(), path, content);
    directory.add_file(file)?;

    let misc = state.misc_mut();
    misc.set_last_calendar(ctype, calendar.to_string());
    if let Err(error) = misc.write_to_file() {
        tracing::warn!(%error, "Unable to remember last calendar");
    }

    Ok(ItemId {
        calendar: calendar.to_string(),
        uid,
        recurrence: None,
    })
}
