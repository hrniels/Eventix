// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_ical::objects::{CalCompType, CalOrganizer, CalTodoStatus, EventLike, PRIORITY_MEDIUM};
use eventix_ical::objects::{CalComponent, CalDate, UpdatableEventLike};
use eventix_ical::parser::ParseError;
use eventix_locale::Locale;
use eventix_state::{CalendarAlarmType, PersonalAlarms};
use std::sync::Arc;
use tracing::warn;

use crate::comps::{
    alarm::AlarmRequest, attendees::Attendees, datetimerange::DateTimeRange, recur::RecurRequest,
    todostatus::TodoStatus,
};
use crate::pages::Page;

/// Maps a DST-related [`ParseError`] to the appropriate localized error message.
fn dst_error_msg<'a>(locale: &'a (dyn Locale + Send + Sync), err: &ParseError) -> &'a str {
    match err {
        ParseError::AmbiguousTime(_) => locale.translate("error.dst_ambiguous"),
        _ => locale.translate("error.dst_nonexistent"),
    }
}

pub trait CompAction {
    fn summary(&self) -> &String;
    fn location(&self) -> &String;
    fn description(&self) -> &String;
    fn rrule(&self) -> Option<&RecurRequest>;
    fn alarm(&self) -> &AlarmRequest;
    fn start_end(&self) -> &DateTimeRange;
    fn attendees(&self) -> Option<&Attendees>;
    fn status(&self) -> Option<&TodoStatus>;

    fn check(
        &self,
        page: &mut Page,
        locale: &Arc<dyn Locale + Send + Sync>,
        ctype: CalCompType,
    ) -> bool {
        if self.summary().is_empty() {
            page.add_error(locale.translate("error.summary_empty"));
            return false;
        }

        let event_tz = self.start_end().effective_timezone(locale);
        let (start, end) = self.start_end().as_caldates(locale, ctype.into());

        if ctype == CalCompType::Event {
            if start.is_none() {
                page.add_error(locale.translate("error.start_datetime"));
                return false;
            }
            if end.is_none() {
                page.add_error(locale.translate("error.end_datetime"));
                return false;
            }
        }

        // validate start/end before using it afterwards (e.g. testing start > end)
        let local_tz = locale.timezone();
        if let Some(ref d) = start
            && let Err(e) = d.validate(local_tz)
        {
            page.add_error(dst_error_msg(locale.as_ref(), &e));
            return false;
        }
        if let Some(ref d) = end
            && let Err(e) = d.validate(local_tz)
        {
            page.add_error(dst_error_msg(locale.as_ref(), &e));
            return false;
        }

        if start.is_some()
            && end.is_some()
            && matches!(start.as_ref().unwrap(), CalDate::Date(..))
                != matches!(end.as_ref().unwrap(), CalDate::Date(..))
        {
            page.add_error(locale.translate("error.time_for_both_or_none"));
            return false;
        }

        if start.is_some() && end.is_some() && start.as_ref().unwrap() > end.as_ref().unwrap() {
            page.add_error(locale.translate("error.end_before_start"));
            return false;
        }

        if let Some(st) = self.status()
            && let Some(completed) = st
                .completed()
                .and_then(|d| d.to_caldate(ctype.into(), false))
            && let Err(e) = completed.validate(local_tz)
        {
            page.add_error(dst_error_msg(locale.as_ref(), &e));
            return false;
        }

        if let Some(rr) = self.rrule()
            && let Err(e) = rr.check_dst(local_tz)
        {
            page.add_error(dst_error_msg(locale.as_ref(), &e));
            return false;
        }

        if self
            .rrule()
            .and_then(|rr| rr.to_rrule().unwrap_or(None))
            .is_some()
            && start.is_none()
        {
            page.add_error(locale.translate("error.repeating_event_start"));
            return false;
        }

        if !self.alarm().check(page, locale, &event_tz) {
            return false;
        }

        true
    }

    fn nonempty_or_none(val: String) -> Option<String> {
        if val.is_empty() { None } else { Some(val) }
    }

    fn update(
        &self,
        calendar: &String,
        cal_alarm_type: &CalendarAlarmType,
        comp: &mut CalComponent,
        personal_alarms: &mut PersonalAlarms,
        organizer: Option<CalOrganizer>,
        locale: &Arc<dyn Locale + Send + Sync>,
    ) -> anyhow::Result<()> {
        let dtype = comp.ctype().into();
        let event_tz = self.start_end().effective_timezone(locale);
        let local_tz = locale.timezone();
        let (start, end) = self.start_end().as_caldates(locale, dtype);

        comp.set_summary(Self::nonempty_or_none(self.summary().clone()));
        comp.set_location(Self::nonempty_or_none(self.location().clone()));
        comp.set_description(Self::nonempty_or_none(self.description().clone()));
        comp.set_start_checked(start, local_tz)?;
        if comp.as_event().is_some() {
            comp.set_end_checked(end, local_tz)?;
        } else {
            comp.set_due_checked(end, local_tz)?;
        }

        let (cal_alarms, pers_alarms) = self.alarm().to_alarms(&event_tz).unwrap();
        if let Some(cal_alarms) = cal_alarms {
            comp.set_alarms(Some(cal_alarms));
        } else {
            comp.set_alarms(None);
        }

        if let CalendarAlarmType::Personal { .. } = cal_alarm_type {
            let pers_cal = personal_alarms.get_or_create(calendar);
            let changed = if let Some(pers_alarms) = pers_alarms {
                pers_cal.set(comp.uid(), comp.rid(), pers_alarms.unwrap_or_default())
            } else {
                pers_cal.unset(comp.uid(), comp.rid())
            };
            if changed && let Err(e) = pers_cal.save() {
                warn!(
                    "Unable to save personal alarms for calendar {}: {}",
                    calendar, e
                );
            }
        }

        if let Some(att) = self.attendees() {
            comp.set_organizer(organizer);
            comp.set_attendees(att.to_cal_attendees());
        } else {
            comp.set_organizer(None);
            comp.set_attendees(None);
        }

        if let Some(td) = comp.as_todo_mut() {
            if let Some(st) = self.status() {
                td.set_status(Some(st.status()));
                if st.status() == CalTodoStatus::Completed {
                    td.set_percent(Some(100));
                    let completed = st.completed().and_then(|d| d.to_caldate(dtype, false));
                    comp.set_completed_checked(completed, local_tz)?;
                } else if st.status() == CalTodoStatus::InProcess {
                    td.set_percent(st.percent());
                } else {
                    td.set_percent(None);
                    td.set_completed(None);
                }
            }
            // set the priority as is required by MS exchange as soon as TODOs are completed -
            // unsure why; we don't care about the priority at the moment and thus are fine with
            // any value.
            comp.set_priority(Some(PRIORITY_MEDIUM));
        }

        comp.set_last_modified(CalDate::now());
        comp.set_stamp(CalDate::now());
        Ok(())
    }
}
