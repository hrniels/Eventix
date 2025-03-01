use std::collections::HashMap;
use std::fs::{self, File};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::DateTime;
use chrono_tz::Tz;

use crate::col::{ColError, Occurrence};
use crate::objects::{
    CalCompType, CalComponent, CalDate, CalDateTime, CalEvent, CalTodo, CalTrigger, Calendar,
    CompDateIterator, CompDateType, EventLike, UpdatableEventLike,
};
use crate::util;

pub struct OccurrenceIterator<'a> {
    file: &'a CalFile,
    start: DateTime<Tz>,
    end: DateTime<Tz>,
    dates: Option<(&'a CalComponent, CompDateIterator<'a>)>,
    seen_rids: Vec<CalDate>,
    // overwritten components and the current index
    sorted_overwritten: Vec<&'a CalComponent>,
    overwritten_index: usize,
    // lookahead candidates for merging
    next_recurrence: Option<Occurrence<'a>>,
    next_overwritten: Option<Occurrence<'a>>,
}

impl<'a> OccurrenceIterator<'a> {
    fn new(
        file: &'a CalFile,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
        dates: Option<(&'a CalComponent, CompDateIterator<'a>)>,
    ) -> Self {
        let mut sorted_overwritten: Vec<&CalComponent> = file.components().iter().collect();
        sorted_overwritten.sort_by_key(|comp| comp.start());
        Self {
            start,
            end,
            file,
            dates,
            sorted_overwritten,
            overwritten_index: 0,
            seen_rids: Vec::new(),
            next_recurrence: None,
            next_overwritten: None,
        }
    }

    fn fetch_next_recurrence(&mut self) -> Option<Occurrence<'a>> {
        // unwrap the base component and the recurring date iterator.
        let (base, ref mut date_iter) = self.dates.as_mut()?;
        for (ty, d, excluded) in date_iter {
            let mut occ = Occurrence::new_single(self.file.source.clone(), base, ty, d, excluded);
            // check if an overwritten event exists for this occurrence.
            if let Some(overwritten) = self.file.cal.components().iter().find(|c| {
                matches!(c.rid(),
                Some(rid)
                    if occ.occurrence_start()
                        == Some(rid.as_start_with_tz(&self.start.timezone())))
            }) {
                let rid = overwritten.rid().unwrap().clone();
                // skip this in case we had it already within the overwritten iterator
                if self.seen_rids.contains(&rid) {
                    continue;
                }
                self.seen_rids.push(rid);

                occ.set_occurrence(overwritten);
                // if it isn't in the range anymore, do not consider it
                if !Self::is_in_range(&occ, self.start, self.end) {
                    continue;
                }
            }
            return Some(occ);
        }
        None
    }

    fn fetch_next_overwritten(&mut self) -> Option<Occurrence<'a>> {
        let base = self.dates.as_ref()?.0;
        let timezone = self.start.timezone();
        while self.overwritten_index < self.sorted_overwritten.len() {
            let overwritten = self.sorted_overwritten[self.overwritten_index];
            self.overwritten_index += 1;
            if let Some(rid) = overwritten.rid() {
                if self.seen_rids.contains(rid) {
                    continue;
                }
                self.seen_rids.push(rid.clone());

                let start_date = overwritten.start().unwrap().as_start_with_tz(&timezone);
                let mut occ = Occurrence::new_single(
                    self.file.source.clone(),
                    base,
                    CompDateType::Start,
                    start_date,
                    false,
                );
                occ.set_occurrence(overwritten);
                if Self::is_in_range(&occ, self.start, self.end) {
                    return Some(occ);
                }
            }
        }
        None
    }

    fn is_in_range(occ: &Occurrence, start: DateTime<Tz>, end: DateTime<Tz>) -> bool {
        let occ_start = occ.occurrence_start().unwrap();
        util::date_ranges_overlap(
            occ_start,
            occ.occurrence_end().unwrap_or(occ_start),
            start,
            end,
        )
    }
}

impl<'a> Iterator for OccurrenceIterator<'a> {
    type Item = Occurrence<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        // fill the lookahead candidates if not already present.
        if self.next_recurrence.is_none() {
            self.next_recurrence = self.fetch_next_recurrence();
        }
        if self.next_overwritten.is_none() {
            self.next_overwritten = self.fetch_next_overwritten();
        }

        // take the earlier one
        match (&self.next_recurrence, &self.next_overwritten) {
            (None, None) => None,
            (Some(_), None) => self.next_recurrence.take(),
            (None, Some(_)) => self.next_overwritten.take(),
            (Some(recurrence), Some(overwritten)) => {
                let rec_start = recurrence.occurrence_start().unwrap();
                let over_start = overwritten.occurrence_start().unwrap();
                if rec_start <= over_start {
                    self.next_recurrence.take()
                } else {
                    self.next_overwritten.take()
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct CalFile {
    source: Arc<String>,
    path: PathBuf,
    cal: Calendar,
}

impl PartialEq for CalFile {
    fn eq(&self, other: &Self) -> bool {
        self.cal == other.cal
    }
}
impl Eq for CalFile {}

impl CalFile {
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

    pub(crate) fn set_source(&mut self, src: Arc<String>) {
        self.source = src;
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub(crate) fn set_path(&mut self, path: PathBuf) {
        self.path = path;
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
                            .map(|(ty, d, excluded)| {
                                Occurrence::new_single(self.source.clone(), first, ty, d, excluded)
                            })
                            .filter(|o| {
                                if o.is_excluded() {
                                    return false;
                                }
                                if let Some(alarm) = o.alarm_date() {
                                    alarm >= start && alarm < end
                                } else {
                                    false
                                }
                            }),
                    );
                }
                CalTrigger::Absolute(date) => {
                    let alarm_date = date.as_start_with_tz(&start.timezone());
                    if alarm_date >= start && alarm_date < end {
                        let fstart = first.start().map(|d| d.as_start_with_tz(&start.timezone()));
                        let fend = first
                            .end_or_due()
                            .map(|d| d.as_end_with_tz(&start.timezone()));
                        alarms.push(Occurrence::new(
                            self.source.clone(),
                            first,
                            fstart,
                            fend,
                            false,
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
                    let mut tmp_occ = Occurrence::new(
                        self.source.clone(),
                        first,
                        Some(rid_tz),
                        None,
                        first.exdates().contains(rid),
                    );
                    tmp_occ.set_occurrence(c);
                    match tmp_occ.alarm_date() {
                        // if the alarm is also within the time frame (and not excluded), just set
                        // the overwritten event
                        Some(alarm) if !tmp_occ.is_excluded() && alarm >= start && alarm < end => {
                            if let Some(occ) = alarms
                                .iter_mut()
                                .find(|o| o.occurrence_start() == Some(rid_tz))
                            {
                                occ.set_occurrence(c);
                            }
                        }
                        // otherwise remove the occurrence from the results
                        _ => alarms.retain(|a| a.occurrence_start() != Some(rid_tz)),
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
        let (fstart, fend, excluded) = match rid {
            Some(rid) => (
                Some(rid.as_start_with_tz(tz)),
                None,
                first.exdates().contains(rid),
            ),
            None => (
                first.start().map(|d| d.as_start_with_tz(tz)),
                first.end_or_due().map(|d| d.as_end_with_tz(tz)),
                false,
            ),
        };
        let mut res = Occurrence::new(self.source.clone(), first, fstart, fend, excluded);

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

    pub fn occurrences_within<F>(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
        filter: F,
    ) -> OccurrenceIterator<'_>
    where
        F: Fn(&CalComponent) -> bool,
    {
        // we currently assume here that there is just a single uid per calendar. that is, if there
        // are multiple events, they all have the same uid and one is the base event with rid =
        // None and the others overwrite specific occurrences of that base event.
        let Some(first) = self.component_with(|c| c.rid().is_none() && filter(c)) else {
            return OccurrenceIterator::new(self, start, end, None);
        };

        OccurrenceIterator::new(
            self,
            start,
            end,
            Some((first, first.dates_within(start, end))),
        )
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

    pub fn create_overwrite<F>(&mut self, rid: CalDate, tz: &Tz, func: F)
    where
        F: FnOnce(&mut CalComponent),
    {
        let base = self
            .components()
            .iter()
            .find(|c| c.rid().is_none())
            .unwrap();

        let mut comp = if base.ctype() == CalCompType::Event {
            CalComponent::Event(CalEvent::new(base.uid()))
        } else {
            CalComponent::Todo(CalTodo::new(base.uid()))
        };

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
        self.cal.add_component(comp);
    }

    pub(crate) fn delete_by_uid<N: AsRef<str>>(&mut self, uid: N) {
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

    struct EventBuilder {
        ev: CalEvent,
    }

    impl EventBuilder {
        fn new<T: ToString>(uid: T) -> Self {
            Self {
                ev: CalEvent::new(uid),
            }
        }
    }

    impl EventBuilder {
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
            self.ev.toggle_exclude(date);
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
        EventBuilder::new(uid)
            .start(CalDate::Date(date, CalCompType::Event.into()))
            .end(CalDate::Date(
                date.succ_opt().unwrap(),
                CalCompType::Event.into(),
            ))
    }

    fn new_file(event: CalEvent) -> CalFile {
        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(event));
        CalFile::new_simple(cal)
    }

    fn new_allday_file(date: NaiveDate, uid: &str) -> CalFile {
        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(new_allday_event(date, uid).done()));
        CalFile::new_simple(cal)
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
    fn files_within_simple() {
        let mut source = CalSource::default();
        source.add(new_allday_file(
            NaiveDate::from_ymd_opt(2024, 10, 2).unwrap(),
            "yes1",
        ));
        source.add(new_allday_file(
            NaiveDate::from_ymd_opt(2024, 10, 1).unwrap(),
            "yes2",
        ));
        source.add(new_allday_file(
            // TODO 2024-10-31 does not work; what does DATE=... mean exactly? doesn't that have a
            // different meaning in different time zones?
            NaiveDate::from_ymd_opt(2024, 10, 30).unwrap(),
            "yes3",
        ));
        source.add(new_allday_file(
            NaiveDate::from_ymd_opt(2023, 10, 31).unwrap(),
            "no1",
        ));
        source.add(new_allday_file(
            NaiveDate::from_ymd_opt(2024, 9, 30).unwrap(),
            "no2",
        ));

        let comps =
            source.occurrences_within(new_date(2024, 10, 1), new_date(2024, 10, 31), |_| true);
        assert!(has_uids(comps, &["yes1", "yes2", "yes3"]));
    }

    #[test]
    fn files_within_no_start() {
        let mut source = CalSource::default();
        source.add(new_file(
            EventBuilder::new("yes1")
                .end(CalDate::Date(
                    NaiveDate::from_ymd_opt(1990, 1, 6).unwrap(),
                    CalCompType::Event.into(),
                ))
                .done(),
        ));
        source.add(new_file(
            EventBuilder::new("yes2")
                .end(CalDate::Date(
                    NaiveDate::from_ymd_opt(1990, 1, 7).unwrap(),
                    CalCompType::Event.into(),
                ))
                .done(),
        ));

        let tz = &chrono_tz::Europe::Berlin;
        let comps =
            source.occurrences_within(new_date(1990, 1, 1), new_date(1990, 1, 31), |_| true);
        assert!(has_uids(comps, &["yes1", "yes2"]));

        let comps =
            source.occurrences_within(new_date(1990, 1, 1), new_date(1990, 1, 31), |_| true);
        let all = comps.collect::<Vec<_>>();
        assert_eq!(all[0].occurrence_start(), None);
        assert_eq!(
            all[0].occurrence_end(),
            Some(
                CalDate::Date(
                    NaiveDate::from_ymd_opt(1990, 1, 6).unwrap(),
                    CalCompType::Event.into()
                )
                .as_end_with_tz(tz)
            )
        );
        assert_eq!(all[1].occurrence_start(), None);
        assert_eq!(
            all[1].occurrence_end(),
            Some(
                CalDate::Date(
                    NaiveDate::from_ymd_opt(1990, 1, 7).unwrap(),
                    CalCompType::Event.into()
                )
                .as_end_with_tz(tz)
            )
        );
        assert_eq!(
            source.occurrence_by_id("yes1", None, tz).unwrap().uid(),
            "yes1"
        );
        assert!(source.occurrence_by_id("not-found", None, tz).is_none());
    }

    #[test]
    fn files_within_missing() {
        let mut source = CalSource::default();
        source.add(new_allday_file(
            NaiveDate::from_ymd_opt(1990, 1, 4).unwrap(),
            "yes1",
        ));
        source.add(new_file(
            EventBuilder::new("yes2")
                .start(CalDate::Date(
                    NaiveDate::from_ymd_opt(1990, 1, 5).unwrap(),
                    CalCompType::Event.into(),
                ))
                .done(),
        ));
        source.add(new_file(
            EventBuilder::new("no1")
                .start(CalDate::Date(
                    NaiveDate::from_ymd_opt(2000, 2, 1).unwrap(),
                    CalCompType::Event.into(),
                ))
                .done(),
        ));
        source.add(new_file(
            EventBuilder::new("no2")
                .start(CalDate::Date(
                    NaiveDate::from_ymd_opt(1988, 2, 1).unwrap(),
                    CalCompType::Event.into(),
                ))
                .end(CalDate::Date(
                    NaiveDate::from_ymd_opt(1989, 12, 31).unwrap(),
                    CalCompType::Event.into(),
                ))
                .done(),
        ));

        let tz = &chrono_tz::Europe::Berlin;
        let comps =
            source.occurrences_within(new_date(1990, 1, 1), new_date(2000, 1, 31), |_| true);
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

        source.add(new_file(
            EventBuilder::new("yes")
                .start(CalDate::Date(
                    NaiveDate::from_ymd_opt(1990, 1, 5).unwrap(),
                    CalCompType::Event.into(),
                ))
                .rrule(rrule)
                .exdate(CalDate::Date(
                    NaiveDate::from_ymd_opt(1990, 1, 7).unwrap(),
                    CalCompType::Event.into(),
                ))
                .exdate(CalDate::Date(
                    NaiveDate::from_ymd_opt(1990, 1, 9).unwrap(),
                    CalCompType::Event.into(),
                ))
                .done(),
        ));

        let occs = source
            .occurrences_within(new_date(1990, 1, 1), new_date(1990, 1, 31), |_| true)
            .filter(|o| !o.is_excluded())
            .collect::<Vec<_>>();
        assert_eq!(occs[0].uid(), "yes");
        assert_eq!(occs[0].occurrence_start(), Some(new_date(1990, 1, 5)));
        assert_eq!(occs[1].occurrence_start(), Some(new_date(1990, 1, 6)));
        assert_eq!(occs[2].occurrence_start(), Some(new_date(1990, 1, 8)));
        assert_eq!(occs[3].occurrence_start(), Some(new_date(1990, 1, 10)));
        assert_eq!(occs[4].occurrence_start(), Some(new_date(1990, 1, 11)));
    }

    #[test]
    fn alarms() {
        let mut source = CalSource::default();
        source.add(new_file(
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
        source.add(new_file(
            new_allday_event(NaiveDate::from_ymd_opt(1990, 1, 4).unwrap(), "id2")
                .alarm(CalAlarm::new(
                    CalAction::Display,
                    CalTrigger::Absolute(CalDate::Date(
                        NaiveDate::from_ymd_opt(1990, 1, 7).unwrap(),
                        CalCompType::Event.into(),
                    )),
                ))
                .done(),
        ));
        source.add(new_file(
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
        source.add(new_file(
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
        source.add(new_file(
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
        assert_eq!(occs[0].occurrence_start(), Some(new_date(1990, 1, 6)));
        assert_eq!(
            occs[0].alarm_date(),
            Some(new_datetime(1990, 1, 5, 23, 50, 0))
        );

        let occs = source
            .due_alarms_within(new_date(1990, 1, 1), new_date(1990, 1, 7))
            .collect::<Vec<_>>();
        assert_eq!(occs.len(), 2);
        assert_eq!(occs[0].uid(), "id1");
        assert_eq!(occs[0].occurrence_start(), Some(new_date(1990, 1, 2)));
        assert_eq!(
            occs[0].alarm_date(),
            Some(new_datetime(1990, 1, 1, 23, 50, 0))
        );
        assert_eq!(occs[1].uid(), "id1");
        assert_eq!(occs[1].occurrence_start(), Some(new_date(1990, 1, 6)));
        assert_eq!(
            occs[1].alarm_date(),
            Some(new_datetime(1990, 1, 5, 23, 50, 0))
        );

        let occs = source
            .due_alarms_within(new_date(1990, 1, 7), new_date(1990, 1, 15))
            .collect::<Vec<_>>();
        assert_eq!(occs.len(), 2);
        assert_eq!(occs[0].uid(), "id2");
        assert_eq!(occs[0].occurrence_start(), Some(new_date(1990, 1, 8)));
        assert_eq!(
            occs[0].alarm_date(),
            Some(new_datetime(1990, 1, 7, 23, 59, 59))
        );
        assert_eq!(occs[1].uid(), "id2");
        assert_eq!(occs[1].occurrence_start(), Some(new_date(1990, 1, 15)));
        assert_eq!(
            occs[1].alarm_date(),
            Some(new_datetime(1990, 1, 14, 23, 59, 59))
        );
    }

    #[test]
    fn alarms_with_recurrence_overwrite() {
        let mut source = CalSource::default();
        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(
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
        cal.add_component(CalComponent::Event(
            new_allday_event(NaiveDate::from_ymd_opt(1990, 1, 6).unwrap(), "id1")
                .rid(CalDate::Date(
                    NaiveDate::from_ymd_opt(1990, 1, 6).unwrap(),
                    CalCompType::Event.into(),
                ))
                .alarm(CalAlarm::new(
                    CalAction::Display,
                    CalTrigger::Relative {
                        related: CalRelated::Start,
                        duration: Duration::hours(1),
                    },
                ))
                .done(),
        ));
        cal.add_component(CalComponent::Event(
            new_allday_event(NaiveDate::from_ymd_opt(1990, 1, 10).unwrap(), "id1")
                .rid(CalDate::Date(
                    NaiveDate::from_ymd_opt(1990, 1, 10).unwrap(),
                    CalCompType::Event.into(),
                ))
                .alarm(CalAlarm::new(
                    CalAction::Display,
                    CalTrigger::Relative {
                        related: CalRelated::End,
                        duration: Duration::days(1),
                    },
                ))
                .done(),
        ));
        source.add(CalFile::new_simple(cal));

        let occs = source
            .due_alarms_within(new_date(1990, 1, 1), new_date(1990, 1, 11))
            .collect::<Vec<_>>();
        assert_eq!(occs.len(), 2);
        assert_eq!(occs[0].uid(), "id1");
        assert_eq!(occs[0].occurrence_start(), Some(new_date(1990, 1, 2)));
        assert_eq!(
            occs[0].alarm_date(),
            Some(new_datetime(1990, 1, 1, 23, 50, 0))
        );
        assert_eq!(occs[1].uid(), "id1");
        assert_eq!(occs[1].occurrence_start(), Some(new_date(1990, 1, 6)));
        assert_eq!(
            occs[1].alarm_date(),
            Some(new_datetime(1990, 1, 6, 1, 0, 0))
        );
    }

    #[test]
    fn recurrence_overwrite_with_date_change() {
        let mut source = CalSource::default();
        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(
            new_allday_event(NaiveDate::from_ymd_opt(1990, 1, 2).unwrap(), "id1")
                .rrule("FREQ=DAILY;INTERVAL=4;COUNT=3".parse().unwrap())
                .done(),
        ));
        cal.add_component(CalComponent::Event(
            new_allday_event(NaiveDate::from_ymd_opt(1990, 1, 8).unwrap(), "id1")
                .rid(CalDate::Date(
                    NaiveDate::from_ymd_opt(1990, 1, 10).unwrap(),
                    CalCompType::Event.into(),
                ))
                .done(),
        ));
        cal.add_component(CalComponent::Event(
            new_allday_event(NaiveDate::from_ymd_opt(1990, 1, 4).unwrap(), "id1")
                .rid(CalDate::Date(
                    NaiveDate::from_ymd_opt(1990, 1, 6).unwrap(),
                    CalCompType::Event.into(),
                ))
                .done(),
        ));
        source.add(CalFile::new_simple(cal));

        // this includes the 6th, but this is overwritten to happen on the 4th, which is outside
        // the range
        let occs = source
            .occurrences_within(new_date(1990, 1, 5), new_date(1990, 1, 7), |_| true)
            .collect::<Vec<_>>();
        assert_eq!(occs.len(), 0);

        // this leads to an empty list from the recurrence itself, but should consider the
        // overwritten one, which is indeed in the requested range.
        let occs = source
            .occurrences_within(new_date(1990, 1, 3), new_date(1990, 1, 9), |_| true)
            .collect::<Vec<_>>();
        assert_eq!(occs.len(), 2);
        assert_eq!(occs[0].uid(), "id1");
        assert_eq!(occs[0].occurrence_start(), Some(new_date(1990, 1, 4)));
        assert_eq!(occs[1].uid(), "id1");
        assert_eq!(occs[1].occurrence_start(), Some(new_date(1990, 1, 8)));
    }
}
