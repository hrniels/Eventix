use std::path::PathBuf;

use chrono::{DateTime, Local, TimeZone, Utc};
use chrono_tz::Tz;
use icalendar::{CalendarComponent, CalendarDateTime, Component, DatePerhapsTime};

use super::Id;

fn ical_datetime_to_utc(ical: CalendarDateTime) -> DateTime<Utc> {
    match ical {
        CalendarDateTime::Utc(dt) => dt,
        CalendarDateTime::WithTimezone {
            date_time: dt,
            tzid,
        } => {
            let tz = if let Ok(tz) = tzid.parse::<Tz>() {
                tz
            } else {
                // we fall back to UTC for all weird values that we see
                Tz::UTC
            };
            tz.from_utc_datetime(&dt).to_utc()
        }
        CalendarDateTime::Floating(dt) => {
            let local = Local.from_utc_datetime(&dt);
            local.to_utc()
        }
    }
}

fn ical_date_to_utc(ical: DatePerhapsTime) -> DateTime<Utc> {
    match ical {
        DatePerhapsTime::Date(date) => Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap()),
        DatePerhapsTime::DateTime(datetime) => ical_datetime_to_utc(datetime),
    }
}

fn is_within(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    ev_start: Option<DatePerhapsTime>,
    ev_end: Option<DatePerhapsTime>,
    rrule: Option<&str>,
) -> bool {
    if let Some(rrule) = rrule {
        println!("FOUND {}", rrule);
        return false;
    }

    if let Some(ev_start) = ev_start {
        if ical_date_to_utc(ev_start) < start {
            return false;
        }
    }
    if let Some(ev_end) = ev_end {
        if ical_date_to_utc(ev_end) > end {
            return false;
        }
    }
    true
}

pub struct CalItem {
    id: Id,
    source: Id,
    path: PathBuf,
    item: icalendar::Calendar,
}

impl CalItem {
    pub fn new(source: Id, path: PathBuf, item: icalendar::Calendar) -> Self {
        Self {
            id: super::generate_id(),
            source,
            path,
            item,
        }
    }

    pub fn id(&self) -> Id {
        self.id
    }

    pub fn source(&self) -> Id {
        self.source
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn item(&self) -> &icalendar::Calendar {
        &self.item
    }

    pub fn items_within(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> impl Iterator<Item = &icalendar::CalendarComponent> {
        self.item.components.iter().filter(move |item| match item {
            CalendarComponent::Event(ev) => is_within(
                start,
                end,
                ev.get_start(),
                ev.get_end(),
                ev.property_value("RRULE"),
            ),
            CalendarComponent::Todo(ev) => is_within(
                start,
                end,
                ev.get_start(),
                ev.get_end(),
                ev.property_value("RRULE"),
            ),
            _ => false,
        })
    }

    pub fn todos(&self) -> impl Iterator<Item = &icalendar::Todo> {
        self.item
            .components
            .iter()
            .filter(|&c| matches!(c, icalendar::CalendarComponent::Todo(_)))
            .map(|t| t.as_todo().unwrap())
    }

    pub fn events(&self) -> impl Iterator<Item = &icalendar::Event> {
        self.item
            .components
            .iter()
            .filter(|&c| matches!(c, icalendar::CalendarComponent::Event(_)))
            .map(|e| e.as_event().unwrap())
    }
}
