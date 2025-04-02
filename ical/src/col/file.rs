use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use chrono::DateTime;
use chrono_tz::Tz;

use crate::col::{AlarmOccurrence, ColError, Occurrence};
use crate::objects::{
    AlarmOverlay, CalCompType, CalComponent, CalDate, CalDateTime, CalEvent, CalTodo, CalTrigger,
    Calendar, CompDateIterator, CompDateType, EventLike, UpdatableEventLike,
};
use crate::util;

/// Iterator that produces occurrences.
///
/// This iterator uses the [`CompDateIterator`] to generate occurrences, but combines these with
/// the overwrites that are present in the used [`CalFile`]. In particular, it ignores occurrences
/// that overwrite the date to be outside of the desired time period and adds occurrences where the
/// overwrite changes the date to be inside of the desired time period.
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
            let mut occ = Occurrence::new_single(self.file.dir.clone(), base, ty, d, excluded);
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

                occ.set_overwrite(overwritten);
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
                    self.file.dir.clone(),
                    base,
                    CompDateType::Start,
                    start_date,
                    false,
                );
                occ.set_overwrite(overwritten);
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

/// A single file containing a [`Calendar`].
///
/// A [`CalFile`] always belongs to a specific [`CalDir`](crate::col::CalDir) and contains exactly
/// one [`Calendar`] (which can contain several [`CalComponent`]s though).
#[derive(Debug)]
pub struct CalFile {
    dir: Arc<String>,
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
            dir: Arc::default(),
            path: PathBuf::default(),
            cal,
        }
    }

    /// Creates a new [`CalFile`] for given directory and path, containing the given calendar.
    pub fn new(dir: Arc<String>, path: PathBuf, cal: Calendar) -> Self {
        Self { dir, path, cal }
    }

    /// Creates a new [`CalFile`] for given directory by reading it from given path.
    pub fn new_from_file(dir: Arc<String>, path: PathBuf) -> Result<Self, ColError> {
        let cal = Self::read_calendar(&path)?;
        Ok(Self::new(dir, path, cal))
    }

    /// Returns the id of the directory this file belongs to.
    pub fn directory(&self) -> &Arc<String> {
        &self.dir
    }

    pub(crate) fn set_directory(&mut self, src: Arc<String>) {
        self.dir = src;
    }

    /// Returns the path of the file this [`CalFile`] is stored in.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub(crate) fn set_path(&mut self, path: PathBuf) {
        self.path = path;
    }

    /// Returns the last modification time of the underlying file.
    pub fn last_modified(&self) -> Result<SystemTime, ColError> {
        let metadata =
            fs::metadata(&self.path).map_err(|_| ColError::FileMetadata(self.path.clone()))?;
        let last_mod = metadata
            .modified()
            .map_err(|_| ColError::FileModified(self.path.clone()))?;
        Ok(last_mod)
    }

    /// Returns the contained [`Calendar`].
    pub fn calendar(&self) -> &Calendar {
        &self.cal
    }

    /// Returns true if any component in the contained [`Calendar`] has the given uid.
    pub fn contains_uid<S: AsRef<str>>(&self, uid: S) -> bool {
        let uid_ref = uid.as_ref();
        self.cal.components().iter().any(|c| c.uid() == uid_ref)
    }

    /// Returns a vector of occurrences whose alarm is due in the given time period.
    ///
    /// Note that excluded occurrences are not returned.
    pub fn due_alarms_between<'o>(
        &'o self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
        overlay: &dyn AlarmOverlay,
    ) -> Vec<AlarmOccurrence<'o>> {
        // this should never happen, but if there is no base component, we're done here
        let Some(first) = self.component_with(|c| c.rid().is_none()) else {
            return vec![];
        };

        // get the alarms for occurrences of the base component
        let mut alarms = vec![];
        if let Some(base_alarms) = overlay.alarms_for_component(first) {
            for alarm in base_alarms {
                match alarm.trigger() {
                    CalTrigger::Relative {
                        related: _,
                        duration,
                    } => {
                        alarms.extend(
                            first
                                .dates_between(start - *duration, end - *duration)
                                .filter_map(|(ty, d, excluded)| {
                                    let occ = Occurrence::new_single(
                                        self.dir.clone(),
                                        first,
                                        ty,
                                        d,
                                        excluded,
                                    );
                                    let aocc = AlarmOccurrence::new(occ, alarm.clone());
                                    match (aocc.occurrence().is_excluded(), aocc.alarm_date()) {
                                        (false, Some(adate)) if adate >= start && adate < end => {
                                            Some(aocc)
                                        }
                                        _ => None,
                                    }
                                }),
                        );
                    }
                    CalTrigger::Absolute(date) => {
                        let alarm_date = date.as_start_with_tz(&start.timezone());
                        if alarm_date >= start && alarm_date < end {
                            let fstart =
                                first.start().map(|d| d.as_start_with_tz(&start.timezone()));
                            let fend = first
                                .end_or_due()
                                .map(|d| d.as_end_with_tz(&start.timezone()));
                            alarms.push(AlarmOccurrence::new(
                                Occurrence::new(self.dir.clone(), first, fstart, fend, false),
                                alarm,
                            ))
                        }
                    }
                }
            }
        }

        // now let's find the alarms for all overwritten components
        if first.is_recurrent() {
            // collect overwritten alarms
            let mut alarm_overwrites = HashMap::new();
            for overwrite in self.cal.components().iter().filter(|c| c.rid().is_some()) {
                // set the overwrite to get the correct summary etc.
                let rid = overwrite.rid().unwrap().clone();
                let rid_tz = rid.as_start_with_tz(&start.timezone());
                if let Some(alarm) = alarms
                    .iter_mut()
                    .find(|a| a.occurrence().occurrence_start() == Some(rid_tz))
                {
                    alarm.occurrence_mut().set_overwrite(overwrite);
                }

                if let Some(alarms) = overwrite.alarms() {
                    alarm_overwrites.insert(rid, alarms);
                }
            }

            // let the overlay customize these overwrites
            let alarm_overwrites = overlay.alarm_overwrites(first, alarm_overwrites);

            for (rid, rid_alarms) in alarm_overwrites {
                // construct a new occurrence
                let rid_tz = rid.as_start_with_tz(&start.timezone());
                let fend = first.duration(&start.timezone()).map(|d| rid_tz + d);
                let mut rid_occ =
                    Occurrence::new(self.dir.clone(), first, Some(rid_tz), fend, false);
                if let Some(overwrite) =
                    self.cal.components().iter().find(|c| c.rid() == Some(&rid))
                {
                    rid_occ.set_overwrite(overwrite);
                }

                // remove all alarms we already had for this occurrence
                alarms.retain(|a| a.occurrence().occurrence_start() != Some(rid_tz));

                // add the desired ones (if they are in the specified time frame)
                for rid_alarm in rid_alarms {
                    let trigger_date = rid_alarm
                        .trigger_date(rid_occ.occurrence_start(), rid_occ.occurrence_end());
                    match trigger_date {
                        Some(alarm) if alarm >= start && alarm < end => {
                            alarms.push(AlarmOccurrence::new(rid_occ.clone(), rid_alarm));
                        }
                        _ => {}
                    }
                }
            }
        }

        alarms
    }

    /// Returns the occurrence with given uid/rid.
    ///
    /// If `rid` is `None`, this method simply returns the base component with the given uid as an
    /// [`Occurrence`], if it does exist. If `rid` is `Some`, it will determine the whether an
    /// overwrite for this specific date (given by the `rid`) exists and if so, it will be
    /// contained in the [`Occurrence`]. The timezone is used to create the date instances in the
    /// returned occurrence.
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
        let mut res = Occurrence::new(self.dir.clone(), first, fstart, fend, excluded);

        if let Some(rid) = rid {
            let occ = self
                .cal
                .components()
                .iter()
                .find(|c| c.uid() == uid.as_ref() && c.rid() == Some(rid));
            if let Some(occ) = occ {
                res.set_overwrite(occ);
            }
        }
        Some(res)
    }

    /// Returns an iterator with all occurrences in the given period of time.
    ///
    /// The filter is used to find the base component and can therefore be leveraged to, for
    /// example, only consider components with a certain uid.
    ///
    /// The returned occurrences are ordered by date. Additionally, overwritten components are
    /// taken into account. That means:
    ///
    /// 1. the overwritten properties will take precedence.
    /// 2. if the overwritten component changes the date to be outside of the period, the
    ///    occurrence will not be delivered by the iterator.
    /// 3. if the overwritten component changes the date to be inside of the period, the occurrence
    ///    will be delivered by the iterator even if the recurrence of the base component is not
    ///    in that period.
    ///
    /// Note that an overlap of the occurrence dates with this period is sufficient. For example,
    /// if an occurrence starts before `end`, but ends after `end`, it will still be delivered by
    /// the iterator.
    ///
    /// Note also that excluded occurrences will be delivered by the iterator, but can be
    /// identified via [`Occurrence::is_excluded`].
    pub fn occurrences_between<F>(
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
            Some((first, first.dates_between(start, end))),
        )
    }

    /// Returns a reference to the component that matches the given filter.
    pub fn component_with<F>(&self, filter: F) -> Option<&CalComponent>
    where
        F: Fn(&CalComponent) -> bool,
    {
        self.cal.components().iter().find(|c| filter(c))
    }

    /// Returns a mutable reference to the component that matches the given filter.
    pub fn component_with_mut<F>(&mut self, filter: F) -> Option<&mut CalComponent>
    where
        F: Fn(&CalComponent) -> bool,
    {
        self.cal.components_mut().iter_mut().find(|c| filter(c))
    }

    /// Returns all components that are part of this file.
    pub fn components(&self) -> &[CalComponent] {
        self.cal.components()
    }

    /// Returns an iterator with all TODOs.
    pub fn todos(&self) -> impl Iterator<Item = &CalTodo> {
        self.components()
            .iter()
            .filter(|&c| c.ctype() == CalCompType::Todo)
            .map(|t| t.as_todo().unwrap())
    }

    /// Returns an iterator with all events.
    pub fn events(&self) -> impl Iterator<Item = &CalEvent> {
        self.components()
            .iter()
            .filter(|&c| c.ctype() == CalCompType::Event)
            .map(|e| e.as_event().unwrap())
    }

    /// Returns a [`HashMap`] with all contacts that occur in this file.
    ///
    /// The key of the hashmap is the address, whereas the value is the common name, if known, or
    /// the address otherwise. The contacts are collected by the list of attendees in all
    /// components.
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

    /// Adds the given component to this file.
    ///
    /// Note that this does not save to file. Please call [`Self::save`] to do so.
    pub fn add_component(&mut self, comp: CalComponent) {
        self.cal.add_component(comp);
    }

    /// Deletes the component with given uid (including overwrites) from this file.
    ///
    /// Note that this does not save to file. Please call [`Self::save`] to do so.
    pub(crate) fn delete_by_uid<N: AsRef<str>>(&mut self, uid: N) {
        self.cal.delete_components(uid);
    }

    /// Creates a new overwrite for the occurrence of the component with given uid at given date.
    ///
    /// The `uid` specifies the id of the base component, whereas the `rid` specifies the date of
    /// the occurrence in UTC. The timezone will be used to for the start date of the occurrence.
    /// The function `func` will be called with a mutable reference to the created overwrite, so
    /// that changes can be made before it is stored.
    ///
    /// Expects that the component with given uid exists, but *not* the overwrite.
    ///
    /// Note that this does not save to file. Please call [`Self::save`] to do so.
    pub fn create_overwrite<F, U>(
        &mut self,
        uid: U,
        rid: CalDate,
        tz: &Tz,
        func: F,
    ) -> Result<(), ColError>
    where
        F: FnOnce(&mut CalComponent),
        U: ToString,
    {
        let uid = uid.to_string();
        let base = self
            .components()
            .iter()
            .find(|c| c.uid() == &uid && c.rid().is_none())
            .ok_or_else(|| ColError::ComponentNotFound(uid.clone()))?;

        // does the overwrite exist?
        if self
            .components()
            .iter()
            .any(|c| c.uid() == &uid && c.rid() == Some(&rid))
        {
            return Err(ColError::RidExists(rid));
        }

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
        Ok(())
    }

    /// Reloads the calendar from file.
    pub fn reload_calendar(&mut self) -> Result<(), ColError> {
        let cal = Self::read_calendar(&self.path)?;
        self.cal = cal;
        Ok(())
    }

    fn read_calendar(path: &Path) -> Result<Calendar, ColError> {
        let mut input = String::new();
        File::open(path)
            .map_err(|e| ColError::FileOpen(path.to_path_buf(), e))?
            .read_to_string(&mut input)
            .map_err(|e| ColError::FileRead(path.to_path_buf(), e))?;

        input
            .parse::<Calendar>()
            .map_err(|e| ColError::FileParse(path.to_path_buf(), e))
    }

    /// Saves the current state to file.
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

    /// Removes this file.
    pub fn remove(&mut self) -> Result<(), ColError> {
        fs::remove_file(&self.path).map_err(|e| ColError::FileRemove(self.path.clone(), e))
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, Duration, NaiveDate, TimeDelta, TimeZone};

    use crate::col::CalDir;
    use crate::objects::{
        CalAction, CalAlarm, CalComponent, CalDate, CalRRule, CalRelated, CalTrigger,
        DefaultAlarmOverlay, UpdatableEventLike,
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
            self.ev.set_alarms(Some(vec![alarm]));
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

    fn ny_datetime(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> DateTime<Tz> {
        chrono_tz::America::New_York
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
    fn files_between_simple() {
        let mut dir = CalDir::default();
        dir.add_file(new_allday_file(
            NaiveDate::from_ymd_opt(2024, 10, 2).unwrap(),
            "yes1",
        ));
        dir.add_file(new_allday_file(
            NaiveDate::from_ymd_opt(2024, 10, 1).unwrap(),
            "yes2",
        ));
        dir.add_file(new_allday_file(
            // TODO 2024-10-31 does not work; what does DATE=... mean exactly? doesn't that have a
            // different meaning in different time zones?
            NaiveDate::from_ymd_opt(2024, 10, 30).unwrap(),
            "yes3",
        ));
        dir.add_file(new_allday_file(
            NaiveDate::from_ymd_opt(2023, 10, 31).unwrap(),
            "no1",
        ));
        dir.add_file(new_allday_file(
            NaiveDate::from_ymd_opt(2024, 9, 30).unwrap(),
            "no2",
        ));

        let comps =
            dir.occurrences_between(new_date(2024, 10, 1), new_date(2024, 10, 31), |_| true);
        assert!(has_uids(comps, &["yes1", "yes2", "yes3"]));
    }

    #[test]
    fn files_between_no_start() {
        let mut dir = CalDir::default();
        dir.add_file(new_file(
            EventBuilder::new("yes1")
                .end(CalDate::Date(
                    NaiveDate::from_ymd_opt(1990, 1, 6).unwrap(),
                    CalCompType::Event.into(),
                ))
                .done(),
        ));
        dir.add_file(new_file(
            EventBuilder::new("yes2")
                .end(CalDate::Date(
                    NaiveDate::from_ymd_opt(1990, 1, 7).unwrap(),
                    CalCompType::Event.into(),
                ))
                .done(),
        ));

        let tz = &chrono_tz::Europe::Berlin;
        let comps = dir.occurrences_between(new_date(1990, 1, 1), new_date(1990, 1, 31), |_| true);
        assert!(has_uids(comps, &["yes1", "yes2"]));

        let comps = dir.occurrences_between(new_date(1990, 1, 1), new_date(1990, 1, 31), |_| true);
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
            dir.occurrence_by_id("yes1", None, tz).unwrap().uid(),
            "yes1"
        );
        assert!(dir.occurrence_by_id("not-found", None, tz).is_none());
    }

    #[test]
    fn files_between_missing() {
        let mut dir = CalDir::default();
        dir.add_file(new_allday_file(
            NaiveDate::from_ymd_opt(1990, 1, 4).unwrap(),
            "yes1",
        ));
        dir.add_file(new_file(
            EventBuilder::new("yes2")
                .start(CalDate::Date(
                    NaiveDate::from_ymd_opt(1990, 1, 5).unwrap(),
                    CalCompType::Event.into(),
                ))
                .done(),
        ));
        dir.add_file(new_file(
            EventBuilder::new("no1")
                .start(CalDate::Date(
                    NaiveDate::from_ymd_opt(2000, 2, 1).unwrap(),
                    CalCompType::Event.into(),
                ))
                .done(),
        ));
        dir.add_file(new_file(
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
        let comps = dir.occurrences_between(new_date(1990, 1, 1), new_date(2000, 1, 31), |_| true);
        assert!(has_uids(comps, &["yes1", "yes2"]));
        assert_eq!(
            dir.occurrence_by_id("yes1", None, tz).unwrap().uid(),
            "yes1"
        );
        assert_eq!(dir.occurrence_by_id("no2", None, tz).unwrap().uid(), "no2");
        assert!(dir.occurrence_by_id("not-found", None, tz).is_none());
    }

    #[test]
    fn recur_with_exdates() {
        let mut dir = CalDir::default();

        let mut rrule = CalRRule::default();
        rrule.set_frequency(crate::objects::CalRRuleFreq::Daily);
        rrule.set_count(7);

        dir.add_file(new_file(
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

        let occs = dir
            .occurrences_between(new_date(1990, 1, 1), new_date(1990, 1, 31), |_| true)
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
        let mut dir = CalDir::default();
        dir.add_file(new_file(
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
        dir.add_file(new_file(
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
        dir.add_file(new_file(
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

        let overlay = DefaultAlarmOverlay::default();
        let alarms = dir
            .due_alarms_between(new_date(1990, 1, 1), new_date(1990, 1, 2), &overlay)
            .collect::<Vec<_>>();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].occurrence().uid(), "id1");
        assert_eq!(alarms[0].alarm_date(), Some(new_date(1990, 1, 1)));

        let alarms = dir
            .due_alarms_between(new_date(1990, 1, 5), new_date(1990, 1, 8), &overlay)
            .collect::<Vec<_>>();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].occurrence().uid(), "id2");
        assert_eq!(alarms[0].alarm_date(), Some(new_date(1990, 1, 7)));
        assert_eq!(alarms[1].occurrence().uid(), "id3");
        assert_eq!(
            alarms[1].alarm_date(),
            Some(new_datetime(1990, 1, 6, 23, 59, 59))
        );
    }

    #[test]
    fn alarms_with_recurrence() {
        let mut dir = CalDir::default();
        dir.add_file(new_file(
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
        dir.add_file(new_file(
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

        let overlay = DefaultAlarmOverlay::default();
        let alarms = dir
            .due_alarms_between(
                new_datetime(1990, 1, 5, 23, 45, 0),
                new_datetime(1990, 1, 5, 23, 55, 0),
                &overlay,
            )
            .collect::<Vec<_>>();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].occurrence().uid(), "id1");
        assert_eq!(
            alarms[0].occurrence().occurrence_start(),
            Some(new_date(1990, 1, 6))
        );
        assert_eq!(
            alarms[0].alarm_date(),
            Some(new_datetime(1990, 1, 5, 23, 50, 0))
        );

        let alarms = dir
            .due_alarms_between(new_date(1990, 1, 1), new_date(1990, 1, 7), &overlay)
            .collect::<Vec<_>>();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].occurrence().uid(), "id1");
        assert_eq!(
            alarms[0].occurrence().occurrence_start(),
            Some(new_date(1990, 1, 2))
        );
        assert_eq!(
            alarms[0].alarm_date(),
            Some(new_datetime(1990, 1, 1, 23, 50, 0))
        );
        assert_eq!(alarms[1].occurrence().uid(), "id1");
        assert_eq!(
            alarms[1].occurrence().occurrence_start(),
            Some(new_date(1990, 1, 6))
        );
        assert_eq!(
            alarms[1].alarm_date(),
            Some(new_datetime(1990, 1, 5, 23, 50, 0))
        );

        let alarms = dir
            .due_alarms_between(new_date(1990, 1, 7), new_date(1990, 1, 15), &overlay)
            .collect::<Vec<_>>();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].occurrence().uid(), "id2");
        assert_eq!(
            alarms[0].occurrence().occurrence_start(),
            Some(new_date(1990, 1, 8))
        );
        assert_eq!(
            alarms[0].alarm_date(),
            Some(new_datetime(1990, 1, 7, 23, 59, 59))
        );
        assert_eq!(alarms[1].occurrence().uid(), "id2");
        assert_eq!(
            alarms[1].occurrence().occurrence_start(),
            Some(new_date(1990, 1, 15))
        );
        assert_eq!(
            alarms[1].alarm_date(),
            Some(new_datetime(1990, 1, 14, 23, 59, 59))
        );
    }

    #[test]
    fn alarms_with_recurrence_overwrite() {
        let mut dir = CalDir::default();
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
        dir.add_file(CalFile::new_simple(cal));

        let overlay = DefaultAlarmOverlay::default();
        let alarms = dir
            .due_alarms_between(new_date(1990, 1, 1), new_date(1990, 1, 11), &overlay)
            .collect::<Vec<_>>();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].occurrence().uid(), "id1");
        assert_eq!(
            alarms[0].occurrence().occurrence_start(),
            Some(new_date(1990, 1, 2))
        );
        assert_eq!(
            alarms[0].alarm_date(),
            Some(new_datetime(1990, 1, 1, 23, 50, 0))
        );
        assert_eq!(alarms[1].occurrence().uid(), "id1");
        assert_eq!(
            alarms[1].occurrence().occurrence_start(),
            Some(new_date(1990, 1, 6))
        );
        assert_eq!(
            alarms[1].alarm_date(),
            Some(new_datetime(1990, 1, 6, 1, 0, 0))
        );

        struct MyOverlay;
        impl AlarmOverlay for MyOverlay {
            fn alarms_for_component(&self, _comp: &CalComponent) -> Option<Vec<CalAlarm>> {
                Some(vec![CalAlarm::new(
                    CalAction::Display,
                    CalTrigger::Relative {
                        related: CalRelated::Start,
                        duration: TimeDelta::hours(1),
                    },
                )])
            }

            fn alarm_overwrites(
                &self,
                _comp: &CalComponent,
                overwrites: HashMap<CalDate, &[CalAlarm]>,
            ) -> HashMap<CalDate, Vec<CalAlarm>> {
                let mut res = HashMap::new();
                for (rid, _alarms) in overwrites {
                    let date = rid.as_naive_date();
                    if date.day() == 2 {
                        // no entry for rid to take the ones from the base component
                    } else if date.day() == 6 {
                        res.insert(rid, vec![]);
                    } else {
                        res.insert(
                            rid,
                            vec![CalAlarm::new(
                                CalAction::Display,
                                CalTrigger::Relative {
                                    related: CalRelated::Start,
                                    duration: -TimeDelta::days(1),
                                },
                            )],
                        );
                    }
                }
                res
            }
        }

        let alarms = dir
            .due_alarms_between(new_date(1990, 1, 1), new_date(1990, 1, 11), &MyOverlay)
            .collect::<Vec<_>>();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].occurrence().uid(), "id1");
        assert_eq!(
            alarms[0].occurrence().occurrence_start(),
            Some(new_date(1990, 1, 2))
        );
        assert_eq!(
            alarms[0].alarm_date(),
            Some(new_datetime(1990, 1, 2, 1, 0, 0))
        );
        // we don't get the alarm for Jan 6, because we disabled it above
        // instead we get the alarm for Jan 10, because we changed it to one day before, so that it
        // falls into that time frame again.
        assert_eq!(alarms[1].occurrence().uid(), "id1");
        assert_eq!(
            alarms[1].occurrence().occurrence_start(),
            Some(new_date(1990, 1, 10))
        );
        assert_eq!(
            alarms[1].alarm_date(),
            Some(new_datetime(1990, 1, 9, 0, 0, 0))
        );
    }

    #[test]
    fn recurrence_overwrite_with_date_change() {
        let mut dir = CalDir::default();
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
        dir.add_file(CalFile::new_simple(cal));

        // this includes the 6th, but this is overwritten to happen on the 4th, which is outside
        // the range
        let occs = dir
            .occurrences_between(new_date(1990, 1, 5), new_date(1990, 1, 7), |_| true)
            .collect::<Vec<_>>();
        assert_eq!(occs.len(), 0);

        // this leads to an empty list from the recurrence itself, but should consider the
        // overwritten one, which is indeed in the requested range.
        let occs = dir
            .occurrences_between(new_date(1990, 1, 3), new_date(1990, 1, 9), |_| true)
            .collect::<Vec<_>>();
        assert_eq!(occs.len(), 2);
        assert_eq!(occs[0].uid(), "id1");
        assert_eq!(occs[0].occurrence_start(), Some(new_date(1990, 1, 4)));
        assert_eq!(occs[1].uid(), "id1");
        assert_eq!(occs[1].occurrence_start(), Some(new_date(1990, 1, 8)));
    }

    #[test]
    fn range_with_local_caldate() {
        let mut dir = CalDir::default();
        let mut cal = Calendar::default();
        let start = NaiveDate::from_ymd_opt(2025, 3, 29)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap();
        let end = start + TimeDelta::hours(1);
        cal.add_component(CalComponent::Event(
            EventBuilder::new("id1")
                .start(CalDate::DateTime(CalDateTime::Timezone(
                    start,
                    "Europe/Berlin".to_string(),
                )))
                .end(CalDate::DateTime(CalDateTime::Timezone(
                    end,
                    "Europe/Berlin".to_string(),
                )))
                .rrule("FREQ=DAILY;COUNT=4".parse().unwrap())
                .done(),
        ));
        dir.add_file(CalFile::new_simple(cal));

        let start = ny_datetime(2025, 3, 29, 0, 0, 0);
        let end = start + TimeDelta::days(7);

        let mut iter = dir.occurrences_between(start, end, |_| true);
        assert_eq!(
            iter.next().unwrap().occurrence_start().unwrap(),
            ny_datetime(2025, 3, 29, 5, 0, 0)
        );
        assert_eq!(
            iter.next().unwrap().occurrence_start().unwrap(),
            ny_datetime(2025, 3, 30, 4, 0, 0)
        );
        assert_eq!(
            iter.next().unwrap().occurrence_start().unwrap(),
            ny_datetime(2025, 3, 31, 4, 0, 0)
        );
        assert_eq!(
            iter.next().unwrap().occurrence_start().unwrap(),
            ny_datetime(2025, 4, 1, 4, 0, 0)
        );
        assert!(iter.next().is_none());
    }

    #[test]
    fn range_with_foreign_caldate() {
        let mut dir = CalDir::default();
        let mut cal = Calendar::default();
        let start = NaiveDate::from_ymd_opt(2025, 3, 29)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap();
        let end = start + TimeDelta::hours(1);
        cal.add_component(CalComponent::Event(
            EventBuilder::new("id1")
                .start(CalDate::DateTime(CalDateTime::Timezone(
                    start,
                    "America/New_York".to_string(),
                )))
                .end(CalDate::DateTime(CalDateTime::Timezone(
                    end,
                    "America/New_York".to_string(),
                )))
                .rrule("FREQ=DAILY;COUNT=4".parse().unwrap())
                .done(),
        ));
        dir.add_file(CalFile::new_simple(cal));

        let start = new_datetime(2025, 3, 29, 0, 0, 0);
        let end = start + TimeDelta::days(7);

        let mut iter = dir.occurrences_between(start, end, |_| true);
        assert_eq!(
            iter.next().unwrap().occurrence_start().unwrap(),
            new_datetime(2025, 3, 29, 15, 0, 0)
        );
        assert_eq!(
            iter.next().unwrap().occurrence_start().unwrap(),
            new_datetime(2025, 3, 30, 16, 0, 0)
        );
        assert_eq!(
            iter.next().unwrap().occurrence_start().unwrap(),
            new_datetime(2025, 3, 31, 16, 0, 0)
        );
        assert_eq!(
            iter.next().unwrap().occurrence_start().unwrap(),
            new_datetime(2025, 4, 1, 16, 0, 0)
        );
        assert!(iter.next().is_none());
    }

    #[test]
    fn range_with_floating_caldate() {
        let mut dir = CalDir::default();
        let mut cal = Calendar::default();
        let start = NaiveDate::from_ymd_opt(2025, 3, 29)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap();
        let end = start + TimeDelta::hours(1);
        cal.add_component(CalComponent::Event(
            EventBuilder::new("id1")
                .start(CalDate::DateTime(CalDateTime::Floating(start)))
                .end(CalDate::DateTime(CalDateTime::Floating(end)))
                .rrule("FREQ=DAILY;COUNT=4".parse().unwrap())
                .done(),
        ));
        dir.add_file(CalFile::new_simple(cal));

        let start = new_datetime(2025, 3, 29, 0, 0, 0);
        let end = start + TimeDelta::days(7);

        let mut iter = dir.occurrences_between(start, end, |_| true);
        assert_eq!(
            iter.next().unwrap().occurrence_start().unwrap(),
            new_datetime(2025, 3, 29, 10, 0, 0)
        );
        assert_eq!(
            iter.next().unwrap().occurrence_start().unwrap(),
            new_datetime(2025, 3, 30, 10, 0, 0)
        );
        assert_eq!(
            iter.next().unwrap().occurrence_start().unwrap(),
            new_datetime(2025, 3, 31, 10, 0, 0)
        );
        assert_eq!(
            iter.next().unwrap().occurrence_start().unwrap(),
            new_datetime(2025, 4, 1, 10, 0, 0)
        );
        assert!(iter.next().is_none());
    }
}
