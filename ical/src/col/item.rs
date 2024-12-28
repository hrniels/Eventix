use std::collections::HashMap;
use std::fs::{self, File};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::DateTime;
use chrono_tz::Tz;

use crate::col::{ColError, Occurrence};
use crate::objects::{
    CalCompType, CalComponent, CalDate, CalDateTime, CalEvent, CalTodo, CalTrigger, Calendar,
    EventLike, UpdatableEventLike,
};

#[derive(Debug)]
pub struct CalItem {
    source: Arc<String>,
    path: PathBuf,
    cal: Calendar,
}

impl PartialEq for CalItem {
    fn eq(&self, other: &Self) -> bool {
        self.cal == other.cal
    }
}
impl Eq for CalItem {}

impl CalItem {
    #[cfg(test)]
    fn new_simple(cal: Calendar) -> Self {
        Self {
            source: Arc::default(),
            path: PathBuf::default(),
            cal,
        }
    }

    pub fn new(source: Arc<String>, path: PathBuf, cal: Calendar) -> Self {
        Self { source, path, cal }
    }

    pub fn source(&self) -> &Arc<String> {
        &self.source
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn calendar(&self) -> &Calendar {
        &self.cal
    }

    pub fn contains_uid<S: AsRef<str>>(&self, uid: S) -> bool {
        let uid_ref = uid.as_ref();
        self.cal.components().iter().any(|c| c.uid() == uid_ref)
    }

    pub fn due_alarms_within(&self, start: DateTime<Tz>, end: DateTime<Tz>) -> Vec<Occurrence<'_>> {
        // this should never happen, but if there is no base component, we're done here
        let Some(first) = self.component_with(|c| c.rid().is_none()) else {
            return vec![];
        };

        // get the alarms for occurrences of the base component
        let mut alarms = vec![];
        if let Some(alarm) = first.alarms().first() {
            match alarm.trigger() {
                CalTrigger::Relative {
                    related: _,
                    duration,
                } => {
                    alarms.extend(
                        first
                            .dates_within(start - *duration, end - *duration)
                            .iter()
                            .map(|d| Occurrence::new(self.source.clone(), first, *d))
                            .filter(|o| {
                                let alarm = o.alarm_date().unwrap();
                                alarm >= start && alarm < end
                            }),
                    );
                }
                CalTrigger::Absolute(date) => {
                    let alarm_date = date.as_start_with_tz(&start.timezone());
                    if alarm_date >= start && alarm_date < end {
                        alarms.push(Occurrence::new(
                            self.source.clone(),
                            first,
                            first.start().unwrap().as_start_with_tz(&start.timezone()),
                        ))
                    }
                }
            }
        }

        // now let's find the alarms for all overwritten components
        if !alarms.is_empty() {
            let overwritten = self.cal.components().iter().filter(|c| c.rid().is_some());
            for c in overwritten {
                if let Some(rid) = c.rid() {
                    let rid_tz = rid.as_start_with_tz(&start.timezone());
                    let mut tmp_occ = Occurrence::new(self.source.clone(), first, rid_tz);
                    tmp_occ.set_occurrence(c);
                    match tmp_occ.alarm_date() {
                        // if the alarm is also within the time frame, just set the overwritten event
                        Some(alarm) if alarm >= start && alarm < end => {
                            if let Some(occ) =
                                alarms.iter_mut().find(|o| o.occurrence_start() == rid_tz)
                            {
                                occ.set_occurrence(c);
                            }
                        }
                        // otherwise remove the occurrence from the results
                        _ => alarms.retain(|a| a.occurrence_start() != rid_tz),
                    }
                }
            }
        }

        alarms
    }

    pub fn occurrence_by_id<S: AsRef<str>>(
        &self,
        uid: S,
        rid: Option<&CalDate>,
        tz: &Tz,
    ) -> Option<Occurrence<'_>> {
        let first = self.component_with(|c| c.rid().is_none() && c.uid() == uid.as_ref())?;

        let date = if let Some(rid) = rid {
            rid.as_start_with_tz(tz)
        } else {
            first.start().unwrap_or(first.stamp()).as_start_with_tz(tz)
        };
        let mut res = Occurrence::new(self.source.clone(), first, date);

        if let Some(rid) = rid {
            let occ = self
                .cal
                .components()
                .iter()
                .find(|c| c.uid() == uid.as_ref() && c.rid() == Some(rid));
            if let Some(occ) = occ {
                res.set_occurrence(occ);
            }
        }
        Some(res)
    }

    pub fn occurrences_within(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
    ) -> Vec<Occurrence<'_>> {
        self.filtered_occurrences_within(start, end, |_| true)
    }

    pub fn filtered_occurrences_within<F>(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
        filter: F,
    ) -> Vec<Occurrence<'_>>
    where
        F: Fn(&CalComponent) -> bool,
    {
        // we currently assume here that there is just a single uid per calendar. that is, if there
        // are multiple events, they all have the same uid and one is the base event with rid =
        // None and the others overwrite specific occurrences of that base event.
        let Some(first) = self.component_with(|c| c.rid().is_none() && filter(c)) else {
            return vec![];
        };

        let mut occs = first
            .dates_within(start, end)
            .iter()
            .map(|d| Occurrence::new(self.source.clone(), first, *d))
            .collect::<Vec<_>>();

        // update occurrences from components that references specific occurrences
        if !occs.is_empty() {
            for c in self.cal.components() {
                if let Some(rid) = c.rid() {
                    let rid_tz = rid.as_start_with_tz(&start.timezone());
                    if let Some(occ) = occs.iter_mut().find(|o| o.occurrence_start() == rid_tz) {
                        occ.set_occurrence(c);
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

    pub fn component_with<F>(&self, filter: F) -> Option<&CalComponent>
    where
        F: Fn(&CalComponent) -> bool,
    {
        self.cal.components().iter().find(|c| filter(c))
    }

    pub fn component_with_mut<F>(&mut self, filter: F) -> Option<&mut CalComponent>
    where
        F: Fn(&CalComponent) -> bool,
    {
        self.cal.components_mut().iter_mut().find(|c| filter(c))
    }

    pub fn components(&self) -> &[CalComponent] {
        self.cal.components()
    }

    pub fn overwrite_component<F>(&mut self, rid: CalDate, tz: &Tz, func: F)
    where
        F: FnOnce(&mut CalComponent),
    {
        let base = self
            .components()
            .iter()
            .filter(|c| c.rid().is_none())
            .next()
            .unwrap();

        let mut comp = if base.ctype() == CalCompType::Event {
            CalComponent::Event(CalEvent::default())
        } else {
            CalComponent::Todo(CalTodo::default())
        };

        comp.set_uid(base.uid().clone());
        let start = CalDate::DateTime(CalDateTime::Timezone(
            rid.as_start_with_tz(tz).naive_local(),
            tz.name().to_string(),
        ));
        comp.set_start(Some(start));
        comp.set_rid(Some(rid));
        comp.set_last_modified(CalDate::now());
        comp.set_stamp(CalDate::now());
        func(&mut comp);
        self.add_component(comp);
    }

    pub fn todos(&self) -> impl Iterator<Item = &CalTodo> {
        self.components()
            .iter()
            .filter(|&c| c.ctype() == CalCompType::Todo)
            .map(|t| t.as_todo().unwrap())
    }

    pub fn events(&self) -> impl Iterator<Item = &CalEvent> {
        self.components()
            .iter()
            .filter(|&c| c.ctype() == CalCompType::Event)
            .map(|e| e.as_event().unwrap())
    }

    pub fn contacts(&self) -> HashMap<String, String> {
        let mut contacts = HashMap::new();
        for c in self.components() {
            if let Some(attendees) = c.attendees() {
                for a in attendees {
                    let cur_name = contacts.get_mut(a.address());
                    match cur_name {
                        Some(cur_name) if a.address() == cur_name && a.common_name().is_some() => {
                            *cur_name = a.common_name().unwrap().clone();
                        }
                        None => {
                            let name = a.common_name().unwrap_or(a.address()).clone();
                            contacts.insert(a.address().clone(), name);
                        }
                        _ => {}
                    }
                }
            }
        }
        contacts
    }

    pub fn add_component(&mut self, comp: CalComponent) {
        self.cal.add(comp);
    }

    pub fn delete_components<N: AsRef<str>>(&mut self, uid: N) {
        self.cal.delete_components(uid);
    }

    pub fn save(&self) -> Result<(), ColError> {
        let file = File::options()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&self.path)
            .map_err(|e| ColError::FileOpen(self.path.clone(), e))?;
        self.cal
            .write(file)
            .map_err(|e| ColError::FileWrite(self.path.clone(), e))
    }

    pub fn remove(&mut self) -> Result<(), ColError> {
        fs::remove_file(&self.path).map_err(|e| ColError::FileRemove(self.path.clone(), e))
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, NaiveDate, TimeZone};

    use crate::col::CalSource;
    use crate::objects::{
        CalAction, CalAlarm, CalComponent, CalDate, CalRRule, CalRelated, CalTrigger,
        UpdatableEventLike,
    };

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
            self.ev.set_start(Some(start));
            self
        }

        fn end(mut self, end: CalDate) -> Self {
            self.ev.set_end(Some(end));
            self
        }

        fn rrule(mut self, rrule: CalRRule) -> Self {
            self.ev.set_rrule(Some(rrule));
            self
        }

        fn rid(mut self, date: CalDate) -> Self {
            self.ev.set_rid(Some(date));
            self
        }

        fn exdate(mut self, date: CalDate) -> Self {
            self.ev.add_exdate(date);
            self
        }

        fn alarm(mut self, alarm: CalAlarm) -> Self {
            self.ev.set_alarms(vec![alarm]);
            self
        }

        fn done(self) -> CalEvent {
            self.ev
        }
    }

    fn new_date(year: i32, month: u32, day: u32) -> DateTime<Tz> {
        new_datetime(year, month, day, 0, 0, 0)
    }

    fn new_datetime(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        min: u32,
        sec: u32,
    ) -> DateTime<Tz> {
        chrono_tz::Europe::Berlin
            .with_ymd_and_hms(year, month, day, hour, min, sec)
            .unwrap()
    }

    fn new_allday_event(date: NaiveDate, uid: &str) -> EventBuilder {
        EventBuilder::default()
            .uid(uid)
            .start(CalDate::Date(date))
            .end(CalDate::Date(date.succ_opt().unwrap()))
    }

    fn new_item(event: CalEvent) -> CalItem {
        let mut cal = Calendar::default();
        cal.add(CalComponent::Event(event));
        CalItem::new_simple(cal)
    }

    fn new_allday_item(date: NaiveDate, uid: &str) -> CalItem {
        let mut cal = Calendar::default();
        cal.add(CalComponent::Event(new_allday_event(date, uid).done()));
        CalItem::new_simple(cal)
    }

    fn has_uids<'a, I: Iterator<Item = Occurrence<'a>>>(result: I, uids: &[&str]) -> bool {
        let result = result.collect::<Vec<_>>();
        assert_eq!(result.len(), uids.len());
        for uid in uids {
            if result.iter().find(|o| o.uid() == *uid).is_none() {
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

        let comps = source.occurrences_within(new_date(2024, 10, 1), new_date(2024, 10, 31));
        assert!(has_uids(comps, &["yes1", "yes2", "yes3"]));
    }

    #[test]
    fn items_within_missing() {
        let mut source = CalSource::default();
        source.add(new_allday_item(
            NaiveDate::from_ymd_opt(1990, 1, 4).unwrap(),
            "yes1",
        ));
        source.add(new_item(
            EventBuilder::default()
                .start(CalDate::Date(NaiveDate::from_ymd_opt(1990, 1, 5).unwrap()))
                .uid("yes2")
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
                .start(CalDate::Date(NaiveDate::from_ymd_opt(1988, 2, 1).unwrap()))
                .end(CalDate::Date(
                    NaiveDate::from_ymd_opt(1989, 12, 31).unwrap(),
                ))
                .uid("no2")
                .done(),
        ));

        let tz = &chrono_tz::Europe::Berlin;
        let comps = source.occurrences_within(new_date(1990, 1, 1), new_date(2000, 1, 31));
        assert!(has_uids(comps, &["yes1", "yes2"]));
        assert_eq!(
            source.occurrence_by_id("yes1", None, tz).unwrap().uid(),
            "yes1"
        );
        assert_eq!(
            source.occurrence_by_id("no2", None, tz).unwrap().uid(),
            "no2"
        );
        assert!(source.occurrence_by_id("not-found", None, tz).is_none());
    }

    #[test]
    fn recur_with_exdates() {
        let mut source = CalSource::default();

        let mut rrule = CalRRule::default();
        rrule.set_frequency(crate::objects::CalRRuleFreq::Daily);
        rrule.set_count(7);

        source.add(new_item(
            EventBuilder::default()
                .start(CalDate::Date(NaiveDate::from_ymd_opt(1990, 1, 5).unwrap()))
                .rrule(rrule)
                .exdate(CalDate::Date(NaiveDate::from_ymd_opt(1990, 1, 7).unwrap()))
                .exdate(CalDate::Date(NaiveDate::from_ymd_opt(1990, 1, 9).unwrap()))
                .uid("yes")
                .done(),
        ));

        let occs = source
            .occurrences_within(new_date(1990, 1, 1), new_date(1990, 1, 31))
            .collect::<Vec<_>>();
        assert_eq!(occs[0].uid(), "yes");
        assert_eq!(occs[0].occurrence_start(), new_date(1990, 1, 5));
        assert_eq!(occs[1].occurrence_start(), new_date(1990, 1, 6));
        assert_eq!(occs[2].occurrence_start(), new_date(1990, 1, 8));
        assert_eq!(occs[3].occurrence_start(), new_date(1990, 1, 10));
        assert_eq!(occs[4].occurrence_start(), new_date(1990, 1, 11));
    }

    #[test]
    fn alarms() {
        let mut source = CalSource::default();
        source.add(new_item(
            new_allday_event(NaiveDate::from_ymd_opt(1990, 1, 3).unwrap(), "id1")
                .alarm(CalAlarm::new(
                    CalAction::Display,
                    CalTrigger::Relative {
                        related: CalRelated::Start,
                        duration: -Duration::days(2),
                    },
                ))
                .done(),
        ));
        source.add(new_item(
            new_allday_event(NaiveDate::from_ymd_opt(1990, 1, 4).unwrap(), "id2")
                .alarm(CalAlarm::new(
                    CalAction::Display,
                    CalTrigger::Absolute(CalDate::Date(
                        NaiveDate::from_ymd_opt(1990, 1, 7).unwrap(),
                    )),
                ))
                .done(),
        ));
        source.add(new_item(
            new_allday_event(NaiveDate::from_ymd_opt(1990, 1, 5).unwrap(), "id3")
                .alarm(CalAlarm::new(
                    CalAction::Display,
                    CalTrigger::Relative {
                        related: CalRelated::End,
                        duration: Duration::days(1),
                    },
                ))
                .done(),
        ));

        let occs = source
            .due_alarms_within(new_date(1990, 1, 1), new_date(1990, 1, 2))
            .collect::<Vec<_>>();
        assert_eq!(occs.len(), 1);
        assert_eq!(occs[0].uid(), "id1");
        assert_eq!(occs[0].alarm_date(), Some(new_date(1990, 1, 1)));

        let occs = source
            .due_alarms_within(new_date(1990, 1, 5), new_date(1990, 1, 8))
            .collect::<Vec<_>>();
        assert_eq!(occs.len(), 2);
        assert_eq!(occs[0].uid(), "id2");
        assert_eq!(occs[0].alarm_date(), Some(new_date(1990, 1, 7)));
        assert_eq!(occs[1].uid(), "id3");
        assert_eq!(
            occs[1].alarm_date(),
            Some(new_datetime(1990, 1, 6, 23, 59, 59))
        );
    }

    #[test]
    fn alarms_with_recurrence() {
        let mut source = CalSource::default();
        source.add(new_item(
            new_allday_event(NaiveDate::from_ymd_opt(1990, 1, 2).unwrap(), "id1")
                .rrule("FREQ=DAILY;INTERVAL=4;COUNT=2".parse().unwrap())
                .alarm(CalAlarm::new(
                    CalAction::Display,
                    CalTrigger::Relative {
                        related: CalRelated::Start,
                        duration: Duration::minutes(-10),
                    },
                ))
                .done(),
        ));
        source.add(new_item(
            new_allday_event(NaiveDate::from_ymd_opt(1990, 1, 8).unwrap(), "id2")
                .rrule("FREQ=WEEKLY".parse().unwrap())
                .alarm(CalAlarm::new(
                    CalAction::Display,
                    CalTrigger::Relative {
                        related: CalRelated::End,
                        duration: -Duration::days(1),
                    },
                ))
                .done(),
        ));

        let occs = source
            .due_alarms_within(
                new_datetime(1990, 1, 5, 23, 45, 0),
                new_datetime(1990, 1, 5, 23, 55, 0),
            )
            .collect::<Vec<_>>();
        assert_eq!(occs.len(), 1);
        assert_eq!(occs[0].uid(), "id1");
        assert_eq!(occs[0].occurrence_start(), new_date(1990, 1, 6));
        assert_eq!(
            occs[0].alarm_date(),
            Some(new_datetime(1990, 1, 5, 23, 50, 0))
        );

        let occs = source
            .due_alarms_within(new_date(1990, 1, 1), new_date(1990, 1, 7))
            .collect::<Vec<_>>();
        assert_eq!(occs.len(), 2);
        assert_eq!(occs[0].uid(), "id1");
        assert_eq!(occs[0].occurrence_start(), new_date(1990, 1, 2));
        assert_eq!(
            occs[0].alarm_date(),
            Some(new_datetime(1990, 1, 1, 23, 50, 0))
        );
        assert_eq!(occs[1].uid(), "id1");
        assert_eq!(occs[1].occurrence_start(), new_date(1990, 1, 6));
        assert_eq!(
            occs[1].alarm_date(),
            Some(new_datetime(1990, 1, 5, 23, 50, 0))
        );

        let occs = source
            .due_alarms_within(new_date(1990, 1, 7), new_date(1990, 1, 15))
            .collect::<Vec<_>>();
        assert_eq!(occs.len(), 2);
        assert_eq!(occs[0].uid(), "id2");
        assert_eq!(occs[0].occurrence_start(), new_date(1990, 1, 8));
        assert_eq!(
            occs[0].alarm_date(),
            Some(new_datetime(1990, 1, 7, 23, 59, 59))
        );
        assert_eq!(occs[1].uid(), "id2");
        assert_eq!(occs[1].occurrence_start(), new_date(1990, 1, 15));
        assert_eq!(
            occs[1].alarm_date(),
            Some(new_datetime(1990, 1, 14, 23, 59, 59))
        );
    }

    #[test]
    fn alarms_with_recurrence_overwrite() {
        let mut source = CalSource::default();
        let mut cal = Calendar::default();
        cal.add(CalComponent::Event(
            new_allday_event(NaiveDate::from_ymd_opt(1990, 1, 2).unwrap(), "id1")
                .rrule("FREQ=DAILY;INTERVAL=4;COUNT=3".parse().unwrap())
                .alarm(CalAlarm::new(
                    CalAction::Display,
                    CalTrigger::Relative {
                        related: CalRelated::Start,
                        duration: Duration::minutes(-10),
                    },
                ))
                .done(),
        ));
        cal.add(CalComponent::Event(
            new_allday_event(NaiveDate::from_ymd_opt(1990, 1, 6).unwrap(), "id1")
                .rid(CalDate::Date(NaiveDate::from_ymd_opt(1990, 1, 6).unwrap()))
                .alarm(CalAlarm::new(
                    CalAction::Display,
                    CalTrigger::Relative {
                        related: CalRelated::Start,
                        duration: Duration::hours(1),
                    },
                ))
                .done(),
        ));
        cal.add(CalComponent::Event(
            new_allday_event(NaiveDate::from_ymd_opt(1990, 1, 10).unwrap(), "id1")
                .rid(CalDate::Date(NaiveDate::from_ymd_opt(1990, 1, 10).unwrap()))
                .alarm(CalAlarm::new(
                    CalAction::Display,
                    CalTrigger::Relative {
                        related: CalRelated::End,
                        duration: Duration::days(1),
                    },
                ))
                .done(),
        ));
        source.add(CalItem::new_simple(cal));

        let occs = source
            .due_alarms_within(new_date(1990, 1, 1), new_date(1990, 1, 11))
            .collect::<Vec<_>>();
        assert_eq!(occs.len(), 2);
        assert_eq!(occs[0].uid(), "id1");
        assert_eq!(occs[0].occurrence_start(), new_date(1990, 1, 2));
        assert_eq!(
            occs[0].alarm_date(),
            Some(new_datetime(1990, 1, 1, 23, 50, 0))
        );
        assert_eq!(occs[1].uid(), "id1");
        assert_eq!(occs[1].occurrence_start(), new_date(1990, 1, 6));
        assert_eq!(
            occs[1].alarm_date(),
            Some(new_datetime(1990, 1, 6, 1, 0, 0))
        );
    }
}
