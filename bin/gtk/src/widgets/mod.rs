// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod date_input;
mod date_picker;
mod month_picker;
mod time_input;
mod time_picker;

pub(crate) use date_input::DateInput;
pub(crate) use date_picker::DatePicker;
pub(crate) use month_picker::MonthPicker;
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
