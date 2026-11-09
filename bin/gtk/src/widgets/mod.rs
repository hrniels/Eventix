// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod alarm_input;
mod calendar_selector;
mod calendar_toggle;
mod color_dot;
mod date_input;
mod date_picker;
mod entry_completion;
mod month_picker;
mod personal_reminder_input;
mod segmented_control;
mod time_input;
mod time_picker;

pub(crate) use alarm_input::AlarmInput;
pub(crate) use calendar_selector::CalendarSelector;
pub(crate) use calendar_toggle::CalendarToggle;
pub(crate) use color_dot::ColorDot;
pub(crate) use date_input::DateInput;
pub(crate) use date_picker::DatePicker;
pub(crate) use entry_completion::EntryCompletion;
pub(crate) use month_picker::MonthPicker;
pub(crate) use personal_reminder_input::PersonalReminderInput;
pub(crate) use segmented_control::SegmentedControl;
pub(crate) use time_input::TimeInput;
pub(crate) use time_picker::TimePicker;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
