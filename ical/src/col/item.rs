use std::path::PathBuf;

use chrono::DateTime;
use chrono_tz::Tz;

use crate::col::{Id, Occurrence};
use crate::objects::{CalComponent, CalEvent, CalTodo, Calendar};

pub struct CalItem {
    id: Id,
    source: Id,
    path: PathBuf,
    cal: Calendar,
}

impl CalItem {
    #[cfg(test)]
    fn new_simple(cal: Calendar) -> Self {
        Self {
            id: super::generate_id(),
            source: 0,
            path: PathBuf::default(),
            cal,
        }
    }

    pub fn new(source: Id, path: PathBuf, cal: Calendar) -> Self {
        Self {
            id: super::generate_id(),
            source,
            path,
            cal,
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

    pub fn calendar(&self) -> &Calendar {
        &self.cal
    }

    pub fn component_by_uid<S: AsRef<str>>(&self, uid: S) -> Option<&CalComponent> {
        self.cal
            .components()
            .iter()
            .find(|c| c.uid() == uid.as_ref() && c.rid().is_none())
    }

    pub fn occurrences_within(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
    ) -> Vec<Occurrence<'_>> {
        let Some(first) = self.cal.components().iter().find(|c| c.rid().is_none()) else {
            return vec![];
        };

        let mut occs = first
            .dates_within(start, end)
            .iter()
            .map(|d| Occurrence::new(self.source, first, *d))
            .collect::<Vec<_>>();

        // update occurrences from components that references specific occurrences
        if !occs.is_empty() {
            for c in self.cal.components() {
                if let Some(rid) = c.rid() {
                    let rid_tz = rid.as_start_with_tz(&start.timezone());
                    if let Some(occ) = occs.iter_mut().find(|o| o.start() == rid_tz) {
                        if let Some(cstart) = c.start() {
                            occ.set_start(cstart.as_start_with_tz(&start.timezone()));
                        }
                        occ.set_component(c);
                    } else {
                        // otherwise this recurrence should be outside of the range
                        assert!(
                            !(rid.as_start_with_tz(&start.timezone()) >= start
                                && rid.as_end_with_tz(&start.timezone()) <= end)
                        );
                    }
                }
            }
        }
        occs
    }

    pub fn todos(&self) -> impl Iterator<Item = &CalTodo> {
        self.cal
            .components()
            .iter()
            .filter(|&c| c.is_todo())
            .map(|t| t.as_todo().unwrap())
    }

    pub fn events(&self) -> impl Iterator<Item = &CalEvent> {
        self.cal
            .components()
            .iter()
            .filter(|&c| c.is_event())
            .map(|e| e.as_event().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone};

    use crate::col::CalSource;
    use crate::objects::{CalComponent, CalDate};

    use super::*;

    #[derive(Default)]
    struct EventBuilder {
        ev: CalEvent,
    }

    impl EventBuilder {
        fn uid(mut self, uid: &str) -> Self {
            self.ev.set_uid(uid);
            self
        }

        fn start(mut self, start: CalDate) -> Self {
            self.ev.set_start(start);
            self
        }

        fn end(mut self, end: CalDate) -> Self {
            self.ev.set_end(end);
            self
        }

        fn done(self) -> CalEvent {
            self.ev
        }
    }

    fn new_date(year: i32, month: u32, day: u32) -> DateTime<Tz> {
        chrono_tz::Europe::Berlin
            .with_ymd_and_hms(year, month, day, 0, 0, 0)
            .unwrap()
    }

    fn new_allday_event(date: NaiveDate, uid: &str) -> CalEvent {
        EventBuilder::default()
            .uid(uid)
            .start(CalDate::Date(date))
            .end(CalDate::Date(date.succ_opt().unwrap()))
            .done()
    }

    fn new_item(event: CalEvent) -> CalItem {
        let mut cal = Calendar::default();
        cal.add(CalComponent::Event(event));
        CalItem::new_simple(cal)
    }

    fn new_allday_item(date: NaiveDate, uid: &str) -> CalItem {
        let mut cal = Calendar::default();
        cal.add(CalComponent::Event(new_allday_event(date, uid)));
        CalItem::new_simple(cal)
    }

    fn has_uids<'a, I: Iterator<Item = Occurrence<'a>>>(result: I, uids: &[&str]) -> bool {
        let result = result.collect::<Vec<_>>();
        assert_eq!(result.len(), uids.len());
        for uid in uids {
            if result
                .iter()
                .find(|o| o.component().as_event().unwrap().uid() == *uid)
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

        let comps = source.components_within(new_date(2024, 10, 1), new_date(2024, 10, 31));
        assert!(has_uids(comps, &["yes1", "yes2", "yes3"]));
    }

    #[test]
    fn items_within_missing() {
        let mut source = CalSource::default();
        source.add(new_allday_item(
            NaiveDate::from_ymd_opt(1990, 1, 4).unwrap(),
            "yes1",
        ));
        source.add(new_item(EventBuilder::default().uid("yes2").done()));
        source.add(new_item(
            EventBuilder::default()
                .start(CalDate::Date(NaiveDate::from_ymd_opt(1990, 1, 5).unwrap()))
                .uid("yes3")
                .done(),
        ));
        source.add(new_item(
            EventBuilder::default()
                .end(CalDate::Date(NaiveDate::from_ymd_opt(1990, 10, 1).unwrap()))
                .uid("yes4")
                .done(),
        ));
        source.add(new_item(
            EventBuilder::default()
                .start(CalDate::Date(NaiveDate::from_ymd_opt(2000, 2, 1).unwrap()))
                .uid("no1")
                .done(),
        ));
        source.add(new_item(
            EventBuilder::default()
                .end(CalDate::Date(
                    NaiveDate::from_ymd_opt(1989, 12, 31).unwrap(),
                ))
                .uid("no2")
                .done(),
        ));

        let comps = source.components_within(new_date(1990, 1, 1), new_date(2000, 1, 31));
        assert!(has_uids(comps, &["yes1", "yes2", "yes3", "yes4"]));
        let uid_yes1 = source.component_by_uid("yes1").unwrap();
        assert_eq!(uid_yes1.uid(), "yes1");
        let uid_yes2 = source.component_by_uid("yes2").unwrap();
        assert_eq!(uid_yes2.uid(), "yes2");
        let uid_no2 = source.component_by_uid("no2").unwrap();
        assert_eq!(uid_no2.uid(), "no2");
        assert!(source.component_by_uid("not-found").is_none());
    }
}
