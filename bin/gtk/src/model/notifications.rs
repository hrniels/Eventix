// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_ical::{col::AlarmOccurrence, objects::EventLike};
use eventix_locale::{DateFlags, Locale};

/// Localized content for a due reminder notification.
pub(crate) struct Notification {
    pub summary: String,
    pub body: String,
}

impl Notification {
    /// Builds localized notification content for a due alarm occurrence.
    pub(crate) fn from_alarm(alarm: &AlarmOccurrence<'_>, locale: &dyn Locale) -> Self {
        let occurrence = alarm.occurrence();
        let mut lines = Vec::new();
        if let Some(start) = occurrence.occurrence_start() {
            lines.push(format!(
                "{}: {}",
                locale.translate("Start"),
                locale.fmt_datetime(&start, DateFlags::None)
            ));
        }
        if let Some(location) = occurrence.location() {
            lines.push(format!("{}: {location}", locale.translate("Location")));
        }
        if let Some(date) = alarm.alarm_date() {
            lines.push(format!(
                "{}: {}",
                locale.translate("Reminder"),
                locale.fmt_datetime(&date, DateFlags::Short)
            ));
        }
        Notification {
            summary: occurrence
                .summary()
                .filter(|summary| !summary.is_empty())
                .cloned()
                .unwrap_or_else(|| locale.translate("Reminder").into()),
            body: lines.join("\n"),
        }
    }
}
