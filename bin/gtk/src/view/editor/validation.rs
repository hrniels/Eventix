// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{NaiveDate, NaiveTime};
use eventix_ical::objects::{CalAlarm, CalCompType, CalDate, CalDateType, CalTrigger, DateContext};
use eventix_locale::Locale;
use eventix_state::actions::items::CreateItem;
use gtk::prelude::*;

use super::{AlarmInput, ItemEditor, recurrence::RecurrenceInput};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
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
    /// Builds a command from parsed inputs or returns all actionable field errors.
    pub fn command(&self) -> Result<Box<CreateItem>, Vec<Issue>> {
        let locale = self.options.locale.as_ref();
        let mut check = Checks {
            locale,
            issues: Vec::new(),
        };

        let calendar = self
            .options
            .calendars
            .get(self.calendar.widget.selected() as usize);
        if calendar.is_none() {
            check.report(Field::Calendar, "error.create_calendar_first");
        }

        if self.summary.text().trim().is_empty() {
            check.report(Field::Summary, "error.summary_empty");
        }

        if !self.new_attendee.text().trim().is_empty() {
            check.report(Field::NewAttendee, "error.new_attendee_not_empty");
        }

        let all_day = self.all_day.is_active();
        let start = self
            .has_start
            .is_active()
            .then(|| check.date(&self.start.entry, Field::StartDate))
            .flatten();
        let end = self
            .has_due
            .is_active()
            .then(|| check.date(&self.end.entry, Field::EndDate))
            .flatten();
        if self.recurrence.frequency.selected() != 0 && !self.has_start.is_active() {
            check.report(Field::StartDate, "error.repeating_event_start");
        }

        let timezone = check.timezone(self.timezone.selected(), all_day);
        let dates = check.event_dates(self, start, end, timezone);
        let until = check.recurrence_until(&self.recurrence, start, all_day, timezone);
        let task_status = self.task_status.as_ref().map(|status| {
            let completed = if status.status.selected() == 1 && status.has_completed.is_active() {
                check
                    .date(&status.completed.entry, Field::CompletedDate)
                    .map(|date| status.completion_date(date))
            } else {
                None
            };
            status.value(completed)
        });

        let alarms = check.alarm(&self.alarm, Field::AlarmDate, Field::AlarmTime, timezone);
        let personal_alarms = if calendar.is_some_and(|calendar| calendar.personal)
            && self.personal_override.is_active()
        {
            Some(check.alarm(
                &self.personal_alarm,
                Field::PersonalAlarmDate,
                Field::PersonalAlarmTime,
                timezone,
            ))
        } else {
            None
        };

        if !check.issues.is_empty() {
            check.issues.sort_by_key(|issue| issue.field);
            return Err(check.issues);
        }

        // Every missing required value has already produced a field issue above.
        let Some(calendar) = calendar else {
            return Err(check.issues);
        };
        let (start, end) = dates;

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
            start,
            end,
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
    issues: Vec<Issue>,
}

impl<'a> Checks<'a> {
    fn timezone(&mut self, selected: u32, all_day: bool) -> &'a chrono_tz::Tz {
        let timezone = chrono_tz::TZ_VARIANTS.get(selected as usize);
        if !all_day && timezone.is_none() {
            self.report(Field::Timezone, "error.valid_date_time");
        }

        // All-day event dates do not use a timezone, but absolute reminders still do.
        timezone.unwrap_or(self.locale.timezone())
    }

    fn event_dates(
        &mut self,
        editor: &ItemEditor,
        start: Option<NaiveDate>,
        end: Option<NaiveDate>,
        timezone: &chrono_tz::Tz,
    ) -> (Option<CalDate>, Option<CalDate>) {
        let all_day = editor.all_day.is_active();
        let (start_time, end_time) = if all_day {
            (None, None)
        } else {
            (
                editor
                    .has_start
                    .is_active()
                    .then(|| self.time(&editor.start_time, Field::StartTime))
                    .flatten(),
                editor
                    .has_due
                    .is_active()
                    .then(|| self.time(&editor.end_time, Field::EndTime))
                    .flatten(),
            )
        };

        if let (Some(start), Some(end)) = (start, end) {
            self.date_range(start, end, start_time, end_time);
        }

        // Check end overflow independently of start parsing so both errors can be shown.
        let event = editor.kind() == CalCompType::Event;
        let exclusive_end = end.and_then(|date| if event { date.succ_opt() } else { Some(date) });
        if all_day && end.is_some() && exclusive_end.is_none() {
            self.report(Field::EndDate, "Please enter a valid date (YYYY-MM-DD).");
        }

        if all_day {
            (
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
            )
        } else {
            (
                start.map(|date| CalDate::new_datetime(date, start_time, timezone.name())),
                end.map(|date| CalDate::new_datetime(date, end_time, timezone.name())),
            )
        }
    }

    fn date_range(
        &mut self,
        start: NaiveDate,
        end: NaiveDate,
        start_time: Option<NaiveTime>,
        end_time: Option<NaiveTime>,
    ) {
        if end < start {
            self.report(Field::EndDate, "error.end_before_start");
        } else if start == end
            && let (Some(start), Some(end)) = (start_time, end_time)
            && end < start
        {
            self.report(Field::EndTime, "error.end_before_start");
        }
    }

    fn recurrence_until(
        &mut self,
        recurrence: &RecurrenceInput,
        start: Option<NaiveDate>,
        all_day: bool,
        timezone: &chrono_tz::Tz,
    ) -> Option<CalDate> {
        if recurrence.frequency.selected() == 0 || recurrence.end.selected() != 2 {
            return None;
        }

        let until = self.date(&recurrence.until.entry, Field::RecurrenceUntil)?;
        if start.is_some_and(|start| until < start) {
            self.report(
                Field::RecurrenceUntil,
                "The recurrence cannot end before the event starts.",
            );
        }

        Some(if all_day {
            CalDate::new_date(until, CalDateType::Inclusive)
        } else {
            let date =
                CalDate::new_datetime(until, NaiveTime::from_hms_opt(23, 59, 59), timezone.name());
            DateContext::system().date_to_utc(&date, timezone)
        })
    }

    fn report(&mut self, field: Field, key: &str) {
        self.issues.push(Issue {
            field,
            message: self.locale.translate(key).to_string(),
        });
    }

    fn date(&mut self, entry: &gtk::Entry, field: Field) -> Option<NaiveDate> {
        match NaiveDate::parse_from_str(&entry.text(), "%Y-%m-%d") {
            Ok(date) => Some(date),
            Err(_) => {
                self.report(field, "Please enter a valid date (YYYY-MM-DD).");
                None
            }
        }
    }

    fn time(&mut self, entry: &gtk::Entry, field: Field) -> Option<NaiveTime> {
        match NaiveTime::parse_from_str(&entry.text(), "%H:%M") {
            Ok(time) => Some(time),
            Err(_) => {
                self.report(field, "Please enter a valid time (HH:MM).");
                None
            }
        }
    }

    fn alarm(
        &mut self,
        input: &AlarmInput,
        date_field: Field,
        time_field: Field,
        timezone: &chrono_tz::Tz,
    ) -> Vec<CalAlarm> {
        let trigger = match input.trigger.selected() {
            1 => input.relative_trigger(),
            2 => {
                let date = self.date(&input.date.entry, date_field);
                let time = self.time(&input.time, time_field);
                let (Some(date), Some(time)) = (date, time) else {
                    return Vec::new();
                };

                let date = CalDate::new_datetime(date, Some(time), timezone.name());
                CalTrigger::Absolute(DateContext::system().date_to_utc(&date, timezone))
            }
            _ => return Vec::new(),
        };

        input.alarms(trigger, self.locale.translate("Reminder"))
    }
}
