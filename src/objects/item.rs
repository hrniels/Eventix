use std::path::PathBuf;

use chrono::DateTime;
use chrono_tz::Tz;
use icalendar::{CalendarComponent, Component, DatePerhapsTime};

use super::{ical_date_to_tz, Id, RecurrenceRule};

fn dates_within(
    start: DateTime<Tz>,
    end: DateTime<Tz>,
    ev_start: Option<DatePerhapsTime>,
    ev_end: Option<DatePerhapsTime>,
    rrule: Option<&str>,
) -> Vec<DateTime<Tz>> {
    if let Some(rrule) = rrule {
        let Some(dtstart) = ev_start else {
            return vec![];
        };
        let rrule = rrule.parse::<RecurrenceRule>().unwrap();
        return rrule.dates_within(ical_date_to_tz(&dtstart, &start.timezone()), start, end);
    }

    match (ev_start, ev_end) {
        (Some(ev_start), Some(ev_end)) => {
            let tzstart = ical_date_to_tz(&ev_start, &start.timezone());
            let tzend = ical_date_to_tz(&ev_end, &start.timezone());
            if tzstart > end || tzend < start {
                vec![]
            } else {
                vec![tzstart]
            }
        }
        (Some(ev_start), None) => {
            let tzstart = ical_date_to_tz(&ev_start, &start.timezone());
            if tzstart > end {
                vec![]
            } else {
                vec![end]
            }
        }
        (None, Some(ev_end)) => {
            let tzend = ical_date_to_tz(&ev_end, &start.timezone());
            if tzend < start {
                vec![]
            } else {
                vec![start]
            }
        }
        (None, None) => vec![start],
    }
}

pub struct CalItem {
    id: Id,
    source: Id,
    path: PathBuf,
    item: icalendar::Calendar,
}

impl CalItem {
    fn new_simple(item: icalendar::Calendar) -> Self {
        Self {
            id: super::generate_id(),
            source: 0,
            path: PathBuf::default(),
            item,
        }
    }

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
    ) -> Vec<(&icalendar::CalendarComponent, DateTime<Tz>)> {
        let Some(first) = self
            .item
            .components
            .iter()
            .find(|c| matches!(c, CalendarComponent::Event(_) | CalendarComponent::Todo(_)))
        else {
            return vec![];
        };

        match first {
            CalendarComponent::Event(ev) => dates_within(
                start,
                end,
                ev.get_start(),
                ev.get_end(),
                ev.property_value("RRULE"),
            ),
            CalendarComponent::Todo(ev) => dates_within(
                start,
                end,
                ev.get_start(),
                ev.get_due(),
                ev.property_value("RRULE"),
            ),
            _ => vec![],
        }
        .iter()
        .map(|d| (first, *d))
        // TODO update/remove this list based on the other items in this calendar
        .collect()
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

    use crate::objects::CalSource;

    use super::*;

    fn new_date(year: i32, month: u32, day: u32) -> DateTime<Tz> {
        chrono_tz::Europe::Berlin
            .with_ymd_and_hms(year, month, day, 0, 0, 0)
            .unwrap()
    }

    fn new_allday_event(date: NaiveDate, uid: &str) -> icalendar::Event {
        icalendar::Event::new().all_day(date).uid(uid).done()
    }

    fn new_item(event: icalendar::Event) -> CalItem {
        CalItem::new_simple(icalendar::Calendar::new().push(event).done())
    }

    fn new_allday_item(date: NaiveDate, uid: &str) -> CalItem {
        CalItem::new_simple(
            icalendar::Calendar::new()
                .push(new_allday_event(date, uid))
                .done(),
        )
    }

    fn has_uids<'a, I: Iterator<Item = (&'a CalendarComponent, DateTime<Tz>)>>(
        result: I,
        uids: &[&str],
    ) -> bool {
        let result = result.collect::<Vec<_>>();
        assert_eq!(result.len(), uids.len());
        for uid in uids {
            if result
                .iter()
                .find(|(c, _date)| c.as_event().unwrap().get_uid().unwrap() == *uid)
                .is_none()
            {
                return false;
            }
        }
        true
    }

    #[test]
    fn items_within_simple() {
        let mut source = CalSource::default();
        source.add(new_allday_item(
            NaiveDate::from_ymd_opt(2024, 10, 2).unwrap(),
            "yes1",
        ));
        source.add(new_allday_item(
            NaiveDate::from_ymd_opt(2024, 10, 1).unwrap(),
            "yes2",
        ));
        source.add(new_allday_item(
            // TODO 2024-10-31 does not work; what does DATE=... mean exactly? doesn't that have a
            // different meaning in different time zones?
            NaiveDate::from_ymd_opt(2024, 10, 30).unwrap(),
            "yes3",
        ));
        source.add(new_allday_item(
            NaiveDate::from_ymd_opt(2023, 10, 31).unwrap(),
            "no1",
        ));
        source.add(new_allday_item(
            NaiveDate::from_ymd_opt(2024, 9, 30).unwrap(),
            "no2",
        ));

        let items = source.items_within(new_date(2024, 10, 1), new_date(2024, 10, 31));
        assert!(has_uids(items, &["yes1", "yes2", "yes3"]));
    }

    #[test]
    fn items_within_missing() {
        let mut source = CalSource::default();
        source.add(new_allday_item(
            NaiveDate::from_ymd_opt(1990, 1, 4).unwrap(),
            "yes1",
        ));
        source.add(new_item(icalendar::Event::new().uid("yes2").done()));
        source.add(new_item(
            icalendar::Event::new()
                .starts(NaiveDate::from_ymd_opt(1990, 1, 5).unwrap())
                .uid("yes3")
                .done(),
        ));
        source.add(new_item(
            icalendar::Event::new()
                .ends(NaiveDate::from_ymd_opt(1990, 10, 1).unwrap())
                .uid("yes4")
                .done(),
        ));
        source.add(new_item(
            icalendar::Event::new()
                .starts(NaiveDate::from_ymd_opt(2000, 2, 1).unwrap())
                .uid("no1")
                .done(),
        ));
        source.add(new_item(
            icalendar::Event::new()
                .ends(NaiveDate::from_ymd_opt(1989, 12, 31).unwrap())
                .uid("no2")
                .done(),
        ));

        let items = source.items_within(new_date(1990, 1, 1), new_date(2000, 1, 31));
        assert!(has_uids(items, &["yes1", "yes2", "yes3", "yes4"]));
    }
}
