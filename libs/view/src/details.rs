// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Owned, localized occurrence details shared by all displayed day segments.

use eventix_ical::{
    col::Occurrence,
    objects::{CalPartStat, CalRole, DateContext, EventLike},
};
use eventix_state::{CalendarAlarmType, State};

/// A contact displayed in an occurrence's details.
#[derive(Clone, Debug)]
pub struct Contact {
    pub name: String,
    pub address: String,
}

/// The response presentation used for invitations and attendee icons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    Accepted,
    Declined,
    Tentative,
    Pending,
}

impl From<CalPartStat> for Response {
    fn from(value: CalPartStat) -> Self {
        match value {
            CalPartStat::Accepted => Self::Accepted,
            CalPartStat::Declined => Self::Declined,
            CalPartStat::Tentative => Self::Tentative,
            _ => Self::Pending,
        }
    }
}

/// An attendee and their localized role and participation status.
#[derive(Clone, Debug)]
pub struct Attendee {
    pub contact: Contact,
    pub required: bool,
    pub response: Response,
    pub tooltip: String,
}

/// Available occurrence actions, respecting ownership and collection permissions.
#[derive(Debug, Default)]
pub struct DetailActions {
    pub delete: bool,
    pub edit: bool,
    pub cancel: bool,
    pub exclude: bool,
    pub series_response: Option<Response>,
    pub occurrence_response: Option<Response>,
    pub read_only: bool,
    pub personal_reminders: bool,
}

/// Localized details for one occurrence, independent of its displayed day segments.
#[derive(Debug)]
pub struct Details {
    pub date_label: &'static str,
    pub date: String,
    pub source_date: Option<String>,
    pub location: String,
    pub description: String,
    pub status: Option<String>,
    pub recurrence: Option<String>,
    pub reminders: Option<String>,
    pub organizer: Option<Contact>,
    pub sent_by: Option<String>,
    pub attendees: Vec<Attendee>,
    pub actions: DetailActions,
}

impl Details {
    pub(crate) fn load(state: &State, occ: &Occurrence<'_>) -> Option<Self> {
        let locale = state.locale();
        let (collection, calendar) = state.settings().calendar(occ.directory())?;
        let mail = collection.email().map(|email| email.address());
        let owner = occ.is_owned_by(mail.as_ref());
        let read_only = collection.is_read_only();
        let personal_reminders = matches!(calendar.alarms(), CalendarAlarmType::Personal { .. });
        let alarms = state
            .personal_alarms()
            .effective_alarms(occ, calendar.alarms());
        let start = occ.occurrence_startdate();
        let end = occ.occurrence_enddate();
        // Locale's range formatter uses one HTML entity for its separator. The snapshot is text.
        let plain = |text: String| text.replace("&#x2012;", "‒");
        let date = plain(locale.date_range(
            start.clone(),
            end.clone(),
            &DateContext::system(),
            locale.timezone(),
        ));
        let source_date = occ.occurrence_range_in_tz(locale.timezone()).map(|range| {
            format!(
                "{} ({})",
                plain(locale.date_range(
                    range.start().cloned(),
                    range.end().cloned(),
                    &DateContext::system(),
                    &range.tz()
                )),
                range.tz_name()
            )
        });
        let mut attendees: Vec<_> = occ
            .attendees()
            .unwrap_or_default()
            .iter()
            .map(|att| {
                let mut tooltip = Vec::new();
                if let Some(role) = att.role() {
                    tooltip.push(format!("{role:?}"));
                }
                if let Some(status) = att.part_stat() {
                    tooltip.push(format!("{status:?}"));
                }
                Attendee {
                    contact: Contact {
                        name: att.common_name().cloned().unwrap_or_else(|| att.address()),
                        address: att.address(),
                    },
                    required: att.role() == Some(CalRole::Required),
                    response: att.part_stat().unwrap_or_default().into(),
                    tooltip: tooltip.join(", "),
                }
            })
            .collect();
        attendees.sort_by(|a, b| {
            (&a.contact.name, &a.contact.address).cmp(&(&b.contact.name, &b.contact.address))
        });
        let series_response = if !owner {
            mail.as_ref()
                .and_then(|mail| occ.base().attendee_status(mail))
                .map(Response::from)
        } else {
            None
        };
        let occurrence_response = if !owner && occ.is_recurrent() {
            mail.as_ref()
                .and_then(|mail| occ.attendee_status(mail))
                .map(Response::from)
        } else {
            None
        };
        Some(Self {
            date_label: match (start.is_some(), end.is_some()) {
                (true, true) => "Date",
                (true, false) => "Start",
                _ => "End",
            },
            date,
            source_date,
            location: occ.location().cloned().unwrap_or_default(),
            description: occ.description().cloned().unwrap_or_default(),
            status: occ
                .event_status()
                .map(|s| locale.translate(&s.to_string()).to_owned()),
            recurrence: occ.rrule().map(|r| r.human(&*locale).to_string()),
            reminders: alarms
                .as_ref()
                .filter(|a| !a.is_empty())
                .map(|alarms| {
                    alarms
                        .iter()
                        .map(|a| a.human(&*locale).to_string())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .or_else(|| personal_reminders.then(|| "−".into())),
            organizer: occ.organizer().map(|org| Contact {
                name: org
                    .common_name()
                    .cloned()
                    .unwrap_or_else(|| org.address().to_owned()),
                address: org.address().to_owned(),
            }),
            sent_by: occ.organizer().and_then(|org| org.sent_by().cloned()),
            attendees,
            actions: DetailActions {
                delete: !read_only,
                edit: !read_only && owner,
                cancel: !read_only
                    && owner
                    && occ.is_recurrent()
                    && occ.organizer().is_some()
                    && !occ.is_cancelled(),
                exclude: !read_only && owner && occ.is_recurrent() && occ.organizer().is_none(),
                series_response,
                occurrence_response,
                read_only,
                personal_reminders,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::NaiveDate;
    use chrono_tz::Europe::Berlin;
    use eventix_ical::col::{CalDir, CalFile};
    use eventix_state::{CalendarSettings, CollectionSettings, EmailAccount, SyncerType};

    use super::{Details, Response};
    use crate::WeekSnapshot;

    fn details(body: &str, mail: &str, read_only: bool, personal: bool) -> Arc<Details> {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        let mut state =
            eventix_state::State::new_with_timezone(Arc::new(xdg), Some(Berlin)).unwrap();
        let mut collection = CollectionSettings::new(SyncerType::VDirSyncer {
            email: EmailAccount::new("User".into(), mail.into()),
            url: "https://example.org".into(),
            read_only,
            username: None,
            password: None,
            time_span: Default::default(),
        });
        let mut calendar = CalendarSettings::default();
        calendar.set_enabled(true);
        if personal {
            calendar.set_alarms(eventix_state::CalendarAlarmType::Personal { default: None });
        }
        collection
            .all_calendars_mut()
            .insert("cal".into(), calendar);
        state
            .settings_mut()
            .collections_mut()
            .insert("col".into(), collection);
        let id = Arc::new("cal".to_owned());
        let mut dir = CalDir::new_empty(id.clone(), tmp.path().into(), "Calendar".into(), false);
        let text = format!(
            "BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VEVENT\nUID:details\n\
             DTSTART;TZID=America/New_York:20261005T090000\n\
             DTEND;TZID=America/New_York:20261005T100000\n{body}\n\
             END:VEVENT\nEND:VCALENDAR\n"
        );
        dir.add_file(CalFile::new(
            id,
            tmp.path().join("event.ics"),
            text.parse().unwrap(),
        ))
        .unwrap();
        state.store_mut().add(dir);
        let week =
            WeekSnapshot::load(&state, NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()).unwrap();
        week.days[0].timed[0].event.details.clone()
    }

    const INVITATION: &str = concat!(
        "ORGANIZER;CN=Organizer;SENT-BY=\"mailto:assistant@example.org\":mailto:owner@example.org\n",
        "ATTENDEE;CN=Zoe;ROLE=OPT-PARTICIPANT;PARTSTAT=TENTATIVE:mailto:zoe@example.org\n",
        "ATTENDEE;CN=Amy;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED:mailto:amy@example.org\n",
        "RRULE:FREQ=DAILY;COUNT=2\nSTATUS:CONFIRMED\n",
        "BEGIN:VALARM\nACTION:DISPLAY\nDESCRIPTION:Reminder\nTRIGGER:-PT15M\nEND:VALARM"
    );

    #[test]
    fn projects_contacts_timezone_recurrence_and_effective_reminders() {
        let details = details(INVITATION, "amy@example.org", false, false);
        assert!(details.date.contains("15:00"));
        let source = details.source_date.as_ref().unwrap();
        assert!(source.contains("09:00") || source.contains("9:00"));
        assert!(source.contains("America/New_York"));
        assert!(details.recurrence.as_ref().unwrap().contains("2"));
        assert_eq!(details.status.as_deref(), Some("Confirmed"));
        assert!(details.reminders.as_ref().unwrap().contains("15"));
        assert_eq!(details.organizer.as_ref().unwrap().name, "Organizer");
        assert_eq!(
            details.sent_by.as_deref(),
            Some("mailto:assistant@example.org")
        );
        assert_eq!(details.attendees[0].contact.name, "Amy");
        assert!(details.attendees[0].required);
        assert_eq!(details.attendees[0].response, Response::Accepted);
        assert_eq!(details.attendees[0].tooltip, "Required, Accepted");
        assert!(!details.attendees[1].required);
        assert_eq!(details.actions.series_response, Some(Response::Accepted));
        assert_eq!(
            details.actions.occurrence_response,
            Some(Response::Accepted)
        );
        assert!(!details.actions.edit);
        assert!(!details.actions.cancel);
        assert!(!details.actions.exclude);
        assert!(details.actions.delete);
    }

    #[test]
    fn ownership_and_read_only_control_actions() {
        let owned = details(INVITATION, "owner@example.org", false, false);
        assert!(owned.actions.edit && owned.actions.delete && owned.actions.cancel);
        assert!(!owned.actions.exclude);
        assert_eq!(owned.actions.series_response, None);
        let read_only = details(INVITATION, "amy@example.org", true, false);
        // Like the HTML UI, the projection respects State's debug-mode writable override.
        assert_eq!(read_only.actions.read_only, !cfg!(debug_assertions));
        assert_eq!(read_only.actions.delete, cfg!(debug_assertions));
        assert!(!read_only.actions.edit && !read_only.actions.cancel);
        assert_eq!(read_only.actions.series_response, Some(Response::Accepted));
        let private = details(
            "RRULE:FREQ=DAILY;COUNT=2",
            "owner@example.org",
            false,
            false,
        );
        assert!(private.actions.exclude && private.actions.edit);
        assert!(!private.actions.cancel);
        let cancelled = details(
            &INVITATION.replace("CONFIRMED", "CANCELLED"),
            "owner@example.org",
            false,
            false,
        );
        assert!(!cancelled.actions.cancel);
    }

    #[test]
    fn personal_reminders_replace_embedded_alarms_and_sparse_fields_stay_absent() {
        let personal = details(INVITATION, "owner@example.org", true, true);
        assert_eq!(personal.reminders.as_deref(), Some("−"));
        assert!(personal.actions.personal_reminders);
        let sparse = details("SUMMARY:Only a title", "owner@example.org", false, false);
        assert!(sparse.attendees.is_empty());
        assert!(sparse.organizer.is_none());
        assert!(sparse.status.is_none());
        assert!(sparse.recurrence.is_none());
        assert!(sparse.reminders.is_none());
        assert!(sparse.actions.edit && sparse.actions.delete);
        assert!(!sparse.actions.exclude && !sparse.actions.cancel);
    }
}
