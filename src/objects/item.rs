use std::path::PathBuf;

use chrono::DateTime;
use chrono_tz::Tz;
use icalendar::{CalendarComponent, Component, DatePerhapsTime};

use super::{ical_date_to_tz, Id};

fn is_within(
    start: DateTime<Tz>,
    end: DateTime<Tz>,
    ev_start: Option<DatePerhapsTime>,
    ev_end: Option<DatePerhapsTime>,
    rrule: Option<&str>,
) -> bool {
    if let Some(rrule) = rrule {
        println!("FOUND {}", rrule);
        return false;
    }

    if let Some(ev_start) = ev_start {
        if ical_date_to_tz(&ev_start, &start.timezone()) > end {
            return false;
        }
    }
    if let Some(ev_end) = ev_end {
        if ical_date_to_tz(&ev_end, &start.timezone()) < start {
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
        start: DateTime<Tz>,
        end: DateTime<Tz>,
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
                ev.get_due(),
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

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone};
    use icalendar::EventLike;

    use super::*;

    fn new_date(year: i32, month: u32, day: u32) -> DateTime<Tz> {
        chrono_tz::Europe::Berlin
            .with_ymd_and_hms(year, month, day, 0, 0, 0)
            .unwrap()
    }

    fn new_allday_event(date: NaiveDate, uid: &str) -> icalendar::Event {
        icalendar::Event::new().all_day(date).uid(uid).done()
    }

    fn has_uids<'a, I: Iterator<Item = &'a CalendarComponent>>(result: I, uids: &[&str]) -> bool {
        let result = result.collect::<Vec<_>>();
        assert_eq!(result.len(), uids.len());
        for uid in uids {
            if result
                .iter()
                .find(|c| c.as_event().unwrap().get_uid().unwrap() == *uid)
                .is_none()
            {
                return false;
            }
        }
        true
    }

    #[test]
    fn items_within_simple() {
        let mut cal = icalendar::Calendar::new();
        cal.push(new_allday_event(
            NaiveDate::from_ymd_opt(2024, 10, 2).unwrap(),
            "yes1",
        ));
        cal.push(new_allday_event(
            NaiveDate::from_ymd_opt(2024, 10, 1).unwrap(),
            "yes2",
        ));
        cal.push(new_allday_event(
            // TODO 2024-10-31 does not work; what does DATE=... mean exactly? doesn't that have a
            // different meaning in different time zones?
            NaiveDate::from_ymd_opt(2024, 10, 30).unwrap(),
            "yes3",
        ));
        cal.push(new_allday_event(
            NaiveDate::from_ymd_opt(2023, 10, 31).unwrap(),
            "no1",
        ));
        cal.push(new_allday_event(
            NaiveDate::from_ymd_opt(2024, 9, 30).unwrap(),
            "no2",
        ));

        let item = CalItem::new(0, "".into(), cal);
        let items = item.items_within(new_date(2024, 10, 1), new_date(2024, 10, 31));
        assert!(has_uids(items, &["yes1", "yes2", "yes3"]));
    }

    #[test]
    fn items_within_missing() {
        let mut cal = icalendar::Calendar::new();
        cal.push(new_allday_event(
            NaiveDate::from_ymd_opt(1990, 1, 4).unwrap(),
            "yes1",
        ));
        cal.push(icalendar::Event::new().uid("yes2").done());
        cal.push(
            icalendar::Event::new()
                .starts(NaiveDate::from_ymd_opt(1990, 1, 5).unwrap())
                .uid("yes3")
                .done(),
        );
        cal.push(
            icalendar::Event::new()
                .ends(NaiveDate::from_ymd_opt(1990, 10, 1).unwrap())
                .uid("yes4")
                .done(),
        );
        cal.push(
            icalendar::Event::new()
                .starts(NaiveDate::from_ymd_opt(2000, 2, 1).unwrap())
                .uid("no1")
                .done(),
        );
        cal.push(
            icalendar::Event::new()
                .ends(NaiveDate::from_ymd_opt(1989, 12, 31).unwrap())
                .uid("no2")
                .done(),
        );

        let item = CalItem::new(0, "".into(), cal);
        let items = item.items_within(new_date(1990, 1, 1), new_date(2000, 1, 31));
        assert!(has_uids(items, &["yes1", "yes2", "yes3", "yes4"]));
    }
}
