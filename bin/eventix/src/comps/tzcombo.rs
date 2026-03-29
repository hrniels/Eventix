// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use askama::Template;
use chrono::{Offset, Utc};
use chrono_tz::TZ_VARIANTS;
use eventix_locale::Locale;
use std::sync::Arc;

use crate::html::filters;

/// A timezone entry with the IANA name, a formatted UTC offset, and the
/// raw value for form submission.
struct TzEntry {
    /// Formatted UTC offset, e.g. `"(UTC+02:00)"`.
    pub offset: String,
    /// Raw IANA timezone name, e.g. `"Europe/Berlin"`.
    pub value: &'static str,
}

/// Formats the UTC offset for the given number of seconds east of UTC.
fn format_offset(offset_secs: i32) -> String {
    let sign = if offset_secs < 0 { '-' } else { '+' };
    let abs = offset_secs.unsigned_abs();
    let hours = abs / 3600;
    let minutes = (abs % 3600) / 60;
    format!("(UTC{sign}{hours:02}:{minutes:02})")
}

/// Returns all timezones as `TzEntry` items, sorted alphabetically by
/// IANA name.
///
/// The offset is computed for the current moment, so it reflects any
/// active DST rules at the time of the call.
fn tz_entries() -> Vec<TzEntry> {
    let now = Utc::now();
    let mut entries: Vec<TzEntry> = TZ_VARIANTS
        .iter()
        .map(|tz| {
            let offset_secs = now.with_timezone(tz).offset().fix().local_minus_utc();
            TzEntry {
                offset: format_offset(offset_secs),
                value: tz.name(),
            }
        })
        .collect();
    entries.sort_by(|a, b| a.value.cmp(b.value));
    entries
}

#[derive(Template)]
#[template(path = "comps/tzcombo.htm")]
pub struct TzComboTemplate {
    locale: Arc<dyn Locale + Send + Sync>,
    name: String,
    id: String,
    selected: String,
}

impl TzComboTemplate {
    pub fn new<N: ToString>(
        locale: Arc<dyn Locale + Send + Sync>,
        name: N,
        selected: String,
    ) -> Self {
        let name = name.to_string();
        Self {
            locale,
            id: name.replace(['[', ']'], "_"),
            name,
            selected,
        }
    }
}
