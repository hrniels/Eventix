// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod calendars;
mod compaction;
mod dayocc;

use anyhow::anyhow;
use eventix_ical::{
    col::CalFile,
    objects::{CalCompType, CalComponent, CalEvent, CalOrganizer, CalTimeZone, CalTodo},
};
use eventix_locale::Locale;
use eventix_state::{CalendarAlarmType, PersonalAlarms};
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;

pub use calendars::{Calendar, Calendars};
pub use compaction::CompAction;
pub use dayocc::{DayOccurrence, OccurrenceOverlap};

pub fn create_component<F>(
    state: &mut eventix_state::State,
    locale: &Arc<dyn Locale + Send + Sync>,
    calendar: &str,
    ctype: CalCompType,
    event_tz: &str,
    populate: F,
) -> anyhow::Result<()>
where
    F: FnOnce(
        &String,
        &CalendarAlarmType,
        &mut CalComponent,
        &mut PersonalAlarms,
        Option<CalOrganizer>,
        &Arc<dyn Locale + Send + Sync>,
    ) -> anyhow::Result<()>,
{
    let calendar = Arc::from(calendar.to_string());
    let (col_settings, cal_settings) = state.settings().calendar(&calendar).unwrap();
    let organizer = col_settings.build_organizer();
    let alarm_type = cal_settings.alarms().clone();

    let uid = Uuid::new_v4();
    let mut comp = if ctype == CalCompType::Event {
        CalComponent::Event(CalEvent::new(uid))
    } else {
        CalComponent::Todo(CalTodo::new(uid))
    };

    populate(
        &calendar,
        &alarm_type,
        &mut comp,
        state.personal_alarms_mut(),
        organizer,
        locale,
    )?;

    let dir = state
        .store_mut()
        .directory_mut(&calendar)
        .ok_or_else(|| anyhow!("Unable to find directory with id {}", calendar))?;

    let mut path = dir.path().clone();
    path.push(format!("{uid}.ics"));

    let mut cal = eventix_ical::objects::Calendar::default();
    // add a VTIMEZONE entry to the calendar with just the name of the timezone to work around a
    // problem in the interaction of davmail and MS exchange. davmail translates the timezones to
    // in ICS files to different names for compatibility reasons (I guess) by taking the timezone
    // from the VTIMEZONE entries and setting the same timezone in all DTSTART, DTEND, etc.
    // properties. If there is no VTIMEZONE entry, this translation isn't done, but davmail inserts
    // a new VTIMEZONE entry, which then has a different timezone name than in DTSTART etc.. This
    // apparently is only a problem when updating ICS files, not when creating them. Therefore, it
    // worked often fine so far, because MS exchange added the VTIMEZONE entry for us. This however
    // does not always happen, so that we run into this issue.
    //
    // A working fix is to add a VTIMEZONE entry with just the timezone name and let davmail/MS
    // exchange add the daylight/standard information for us.
    cal.add_timezone(CalTimeZone::new(event_tz.to_string()));

    let mut file = CalFile::new(calendar.clone(), path, cal);

    file.add_component(comp);
    file.save()?;

    dir.add_file(file);

    // remember the last used calendar
    let misc = state.misc_mut();
    misc.set_last_calendar(ctype, calendar.to_string());
    if let Err(e) = misc.write_to_file() {
        warn!("Unable to misc state: {}", e);
    }

    Ok(())
}
