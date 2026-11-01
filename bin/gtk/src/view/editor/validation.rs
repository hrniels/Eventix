// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{NaiveDate, NaiveTime};
use eventix_ical::objects::{CalAlarm, CalCompType, CalDate, CalDateType, CalTrigger, DateContext};
use eventix_locale::Locale;
use eventix_state::actions::items::CreateItem;
use gtk::prelude::*;

use super::{AlarmInput, ItemEditor, recurrence::RecurrenceInput};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Field {
    Calendar,
    Summary,
    CompletedDate,
    StartDate,
    StartTime,
    EndDate,
    EndTime,
    Timezone,
    AlarmDate,
    AlarmTime,
    PersonalAlarmDate,
    PersonalAlarmTime,
    RecurrenceUntil,
    NewAttendee,
}

#[derive(Debug)]
pub(crate) struct Issue {
    pub field: Field,
    pub message: String,
}

impl ItemEditor {
    /// Builds a command from parsed inputs or returns the first invalid field.
    pub fn command(&self) -> Result<Box<CreateItem>, Issue> {
        let locale = self.options.locale.as_ref();
        let check = Checks { locale };

        let calendar = self
            .options
            .calendars
            .get(self.calendar.widget.selected() as usize)
            .ok_or_else(|| check.issue(Field::Calendar, "error.create_calendar_first"))?;

        if self.summary.text().trim().is_empty() {
            return Err(check.issue(Field::Summary, "error.summary_empty"));
        }

        let task_status = self
            .task_status
            .as_ref()
            .map(|status| {
                let completed = if status.status.selected() == 1 && status.has_completed.is_active()
                {
                    Some(status.completion_date(
                        check.date(&status.completed.entry, Field::CompletedDate)?,
                    ))
                } else {
                    None
                };
                Ok(status.value(completed))
            })
            .transpose()?;

        let all_day = self.all_day.is_active();
        let start = self
            .has_start
            .is_active()
            .then(|| check.date(&self.start.entry, Field::StartDate))
            .transpose()?;
        let end = self
            .has_due
            .is_active()
            .then(|| check.date(&self.end.entry, Field::EndDate))
            .transpose()?;
        if self.recurrence.frequency.selected() != 0 && !self.has_start.is_active() {
            return Err(check.issue(Field::StartDate, "error.repeating_event_start"));
        }

        let timezone = check.timezone(self.timezone.selected(), all_day)?;
        let (start_date, end_date) = check.event_dates(self, start, end, timezone)?;

        let alarms = check.alarm(&self.alarm, Field::AlarmDate, Field::AlarmTime, timezone)?;
        let personal_alarms = if calendar.personal && self.personal_override.is_active() {
            Some(check.alarm(
                &self.personal_alarm,
                Field::PersonalAlarmDate,
                Field::PersonalAlarmTime,
                timezone,
            )?)
        } else {
            None
        };

        let until = check.recurrence_until(&self.recurrence, start, all_day, timezone)?;
        if !self.new_attendee.text().trim().is_empty() {
            return Err(check.issue(Field::NewAttendee, "error.new_attendee_not_empty"));
        }

        let buffer = self.description.buffer();
        let mut command = Box::new(CreateItem {
            kind: self.kind(),
            task_status,
            calendar: calendar.id.clone(),
            summary: self.summary.text().to_string(),
            location: self.location.text().to_string(),
            description: buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), true)
                .to_string(),
            start: start_date,
            end: end_date,
            recurrence: self.recurrence.value(until),
            alarms,
            personal_alarms,
            attendees: self
                .attendees
                .borrow()
                .iter()
                .filter(|attendee| attendee.widget.get_visible())
                .map(super::AttendeeInput::value)
                .collect(),
        });

        // Preserve precision, custom recurrence clauses, and multiple/extended alarms when
        // their visible controls have not changed.
        if let (Some(existing), Some(initial)) = (
            self.existing.borrow().as_ref(),
            self.initial.borrow().as_ref(),
        ) {
            let original = &existing.fields;
            if command.start == initial.start {
                command.start = original.start.clone();
            }

            if command.end == initial.end {
                command.end = original.end.clone();
            }

            if command.recurrence == initial.recurrence {
                command.recurrence = original.recurrence.clone();
            }

            if command.alarms == initial.alarms {
                command.alarms = original.alarms.clone();
            }

            if command.personal_alarms == initial.personal_alarms {
                command.personal_alarms = original.personal_alarms.clone();
            }

            if command.task_status == initial.task_status {
                command.task_status = original.task_status.clone();
            }
        }

        Ok(command)
    }
}

struct Checks<'a> {
    locale: &'a dyn Locale,
}

impl<'a> Checks<'a> {
    fn timezone(&self, selected: u32, all_day: bool) -> Result<&'a chrono_tz::Tz, Issue> {
        let timezone = chrono_tz::TZ_VARIANTS.get(selected as usize);
        if !all_day && timezone.is_none() {
            return Err(self.issue(Field::Timezone, "error.valid_date_time"));
        }

        // All-day event dates do not use a timezone, but absolute reminders still do.
        Ok(timezone.unwrap_or(self.locale.timezone()))
    }

    fn event_dates(
        &self,
        editor: &ItemEditor,
        start: Option<NaiveDate>,
        end: Option<NaiveDate>,
        timezone: &chrono_tz::Tz,
    ) -> Result<(Option<CalDate>, Option<CalDate>), Issue> {
        let all_day = editor.all_day.is_active();
        let (start_time, end_time) = if all_day {
            (None, None)
        } else {
            (
                editor
                    .has_start
                    .is_active()
                    .then(|| self.time(&editor.start_time, Field::StartTime))
                    .transpose()?,
                editor
                    .has_due
                    .is_active()
                    .then(|| self.time(&editor.end_time, Field::EndTime))
                    .transpose()?,
            )
        };

        if let (Some(start), Some(end)) = (start, end) {
            self.date_range(start, end, start_time, end_time)?;
        }

        let event = editor.kind() == CalCompType::Event;
        let exclusive_end = end.and_then(|date| if event { date.succ_opt() } else { Some(date) });
        if all_day && end.is_some() && exclusive_end.is_none() {
            return Err(self.issue(Field::EndDate, "Please enter a valid date (YYYY-MM-DD)."));
        }

        if all_day {
            Ok((
                start.map(|date| CalDate::new_date(date, CalDateType::Inclusive)),
                exclusive_end.map(|date| {
                    CalDate::new_date(
                        date,
                        if event {
                            CalDateType::Exclusive
                        } else {
                            CalDateType::Inclusive
                        },
                    )
                }),
            ))
        } else {
            Ok((
                start.map(|date| CalDate::new_datetime(date, start_time, timezone.name())),
                end.map(|date| CalDate::new_datetime(date, end_time, timezone.name())),
            ))
        }
    }

    fn date_range(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        start_time: Option<NaiveTime>,
        end_time: Option<NaiveTime>,
    ) -> Result<(), Issue> {
        if end < start {
            return Err(self.issue(Field::EndDate, "error.end_before_start"));
        } else if start == end
            && let (Some(start), Some(end)) = (start_time, end_time)
            && end < start
        {
            return Err(self.issue(Field::EndTime, "error.end_before_start"));
        }
        Ok(())
    }

    fn recurrence_until(
        &self,
        recurrence: &RecurrenceInput,
        start: Option<NaiveDate>,
        all_day: bool,
        timezone: &chrono_tz::Tz,
    ) -> Result<Option<CalDate>, Issue> {
        if recurrence.frequency.selected() == 0 || recurrence.end.selected() != 2 {
            return Ok(None);
        }

        let until = self.date(&recurrence.until.entry, Field::RecurrenceUntil)?;
        if start.is_some_and(|start| until < start) {
            return Err(self.issue(
                Field::RecurrenceUntil,
                "The recurrence cannot end before the event starts.",
            ));
        }

        Ok(Some(if all_day {
            CalDate::new_date(until, CalDateType::Inclusive)
        } else {
            let date =
                CalDate::new_datetime(until, NaiveTime::from_hms_opt(23, 59, 59), timezone.name());
            DateContext::system().date_to_utc(&date, timezone)
        }))
    }

    fn issue(&self, field: Field, key: &str) -> Issue {
        Issue {
            field,
            message: self.locale.translate(key).to_string(),
        }
    }

    fn date(&self, entry: &gtk::Entry, field: Field) -> Result<NaiveDate, Issue> {
        NaiveDate::parse_from_str(&entry.text(), "%Y-%m-%d")
            .map_err(|_| self.issue(field, "Please enter a valid date (YYYY-MM-DD)."))
    }

    fn time(&self, entry: &gtk::Entry, field: Field) -> Result<NaiveTime, Issue> {
        NaiveTime::parse_from_str(&entry.text(), "%H:%M")
            .map_err(|_| self.issue(field, "Please enter a valid time (HH:MM)."))
    }

    fn alarm(
        &self,
        input: &AlarmInput,
        date_field: Field,
        time_field: Field,
        timezone: &chrono_tz::Tz,
    ) -> Result<Vec<CalAlarm>, Issue> {
        let trigger = match input.trigger.selected() {
            1 => input.relative_trigger(),
            2 => {
                let date = self.date(&input.date.entry, date_field)?;
                let time = self.time(&input.time, time_field)?;

                let date = CalDate::new_datetime(date, Some(time), timezone.name());
                CalTrigger::Absolute(DateContext::system().date_to_utc(&date, timezone))
            }
            _ => return Ok(Vec::new()),
        };

        Ok(input.alarms(trigger, self.locale.translate("Reminder")))
    }
}
