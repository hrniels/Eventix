// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_ical::objects::{CalCompType, CalDate, CalRRule};

pub struct ImportCalendar {
    pub id: String,
    pub name: String,
    pub color: String,
    pub types: Vec<CalCompType>,
}

pub struct ImportComponent {
    pub ty: CalCompType,
    pub summary: Option<String>,
    pub start: Option<CalDate>,
    pub end: Option<CalDate>,
    pub rrule: Option<CalRRule>,
    pub exists_in: Option<(String, String)>,
}

pub struct ImportModel {
    pub calendars: Vec<ImportCalendar>,
    pub items: Vec<ImportComponent>,
}

impl ImportModel {
    pub fn new(calendars: Vec<ImportCalendar>, items: Vec<ImportComponent>) -> Self {
        Self { calendars, items }
    }
}
