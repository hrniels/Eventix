// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use tracing::info;

use crate::col::{AlarmOccurrence, ColError, Occurrence};
use crate::objects::{
    AlarmOverlay, CalCompType, CalComponent, CalDate, CalDateTime, CalEvent, CalTodo, CalTrigger,
    Calendar, CompDateIterator, CompDateType, EventLike, ResolvedDateTime, UpdatableEventLike,
};
use crate::util;

/// Iterator that produces occurrences.
///
/// This iterator uses the [`CompDateIterator`] to generate occurrences, but combines these with
/// the overwrites that are present in the used [`CalFile`]. In particular, it ignores occurrences
/// that overwrite the date to be outside of the desired time period and adds occurrences where the
/// overwrite changes the date to be inside of the desired time period.
pub struct OccurrenceIterator<'a, 'r> {
    file: &'a CalFile,
    start: DateTime<Tz>,
    end: DateTime<Tz>,
    dates: Option<(&'a CalComponent, CompDateIterator<'a, 'r>)>,
    seen_rids: Vec<CalDate>,
    // overwritten components and the current index
    sorted_overwritten: Vec<&'a CalComponent>,
    overwritten_index: usize,
    // lookahead candidates for merging
    next_recurrence: Option<Occurrence<'a>>,
    next_overwritten: Option<Occurrence<'a>>,
}

impl<'a, 'r> OccurrenceIterator<'a, 'r> {
    fn new(
        file: &'a CalFile,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
        dates: Option<(&'a CalComponent, CompDateIterator<'a, 'r>)>,
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
        let (base, date_iter) = self.dates.as_mut()?;
        let resolver = self.file.cal.timezone_resolver();
        for (ty, d, excluded) in date_iter {
            let mut occ = Occurrence::new_single_in_tz(
                self.file.dir.clone(),
                base,
                ty,
                d,
                excluded,
                self.start.timezone(),
            );
            // check if an overwritten event exists for this occurrence.
            if let Some(overwritten) = self.file.cal.components().iter().find(|c| {
                matches!(c.rid(),
                Some(rid)
                    if occ.resolved_occurrence_start()
                        == Some(rid.as_start_with_resolver(&self.start.timezone(), &resolver)))
            }) {
                let rid = overwritten.rid().unwrap().clone();
                // skip this in case we had it already within the overwritten iterator
                if self.seen_rids.contains(&rid) {
                    continue;
                }
                self.seen_rids.push(rid);

                occ.set_overwrite(overwritten, &self.start.timezone(), &resolver);
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
        let resolver = self.file.cal.timezone_resolver();
        while self.overwritten_index < self.sorted_overwritten.len() {
            let overwritten = self.sorted_overwritten[self.overwritten_index];
            self.overwritten_index += 1;
            if let Some(rid) = overwritten.rid() {
                if self.seen_rids.contains(rid) {
                    continue;
                }
                self.seen_rids.push(rid.clone());

                let start_date = overwritten
                    .start()
                    .unwrap()
                    .as_start_with_resolver(&timezone, &resolver);
                let mut occ = Occurrence::new_single_in_tz(
                    self.file.dir.clone(),
                    base,
                    CompDateType::Start,
                    start_date,
                    base.exdates()
                        .iter()
                        .map(|d| d.as_start_with_resolver(&timezone, &resolver))
                        .any(|d| d == rid.as_start_with_resolver(&timezone, &resolver)),
                    timezone,
                );
                occ.set_overwrite(overwritten, &timezone, &resolver);
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
            occ_start.with_timezone(&Utc),
            occ.occurrence_end()
                .unwrap_or(occ_start)
                .with_timezone(&Utc),
            start,
            end,
        )
    }
}

fn resolved_in_range(resolved: ResolvedDateTime, start: DateTime<Tz>, end: DateTime<Tz>) -> bool {
    let resolved = resolved.with_timezone(&Utc);
    let start = start.with_timezone(&Utc);
    let end = end.with_timezone(&Utc);
    resolved >= start && resolved < end
}

impl<'a, 'r> Iterator for OccurrenceIterator<'a, 'r> {
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
                let rec_start = recurrence.resolved_occurrence_start().unwrap();
                let over_start = overwritten.resolved_occurrence_start().unwrap();
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
    ///
    /// Note that this method assumes that all calendar components in this file have the same UID.
    /// After parsing, all component dates are validated against `local_tz`. Components with times
    /// falling in a DST gap (non-existent) or DST fold (ambiguous) are removed with a warning.
    pub fn new_from_file(dir: Arc<String>, path: PathBuf, local_tz: &Tz) -> Result<Self, ColError> {
        let cal = Self::read_calendar(&path, local_tz)?;
        Ok(Self::new(dir, path, cal))
    }

    /// Creates multiple [`CalFile`]s from given external file.
    ///
    /// for given directory, one for each UID.
    ///
    /// In contrast to [`Self::new_from_file`], the given `path` is assumed to be outside of the
    /// directory and may contain components with different UIDs in the same file. For that reason,
    /// potentially multiple [`CalFile`] instances are created and one file per UID is created in
    /// the given directory path `dir_path`.
    ///
    /// After parsing, all component dates are validated against `local_tz`. Components with times
    /// falling in a DST gap (non-existent) or DST fold (ambiguous) are removed with a warning.
    pub fn new_from_external_file(
        dir: Arc<String>,
        dir_path: PathBuf,
        path: PathBuf,
        local_tz: &Tz,
    ) -> Result<Vec<CalFile>, ColError> {
        let cal = Self::read_calendar(&path, local_tz)?;
        let cals = cal.split_by_uid();
        Ok(cals
            .into_iter()
            .map(|cal| {
                let mut new_path = dir_path.clone();
                new_path.push(format!("{}.ics", cal.components().first().unwrap().uid()));
                Self::new(dir.clone(), new_path, cal)
            })
            .collect())
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

    /// Returns a mutable reference to the the contained [`Calendar`].
    pub fn calendar_mut(&mut self) -> &mut Calendar {
        &mut self.cal
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
        let resolver = self.cal.timezone_resolver();
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
                            self.occurrences_between(start - **duration, end - **duration, |_| {
                                true
                            })
                            .filter_map(|occ| {
                                let aocc = AlarmOccurrence::new(occ, alarm.clone());
                                match (aocc.occurrence().is_excluded(), aocc.resolved_alarm_date())
                                {
                                    (false, Some(adate))
                                        if resolved_in_range(adate, start, end) =>
                                    {
                                        Some(aocc)
                                    }
                                    _ => None,
                                }
                            }),
                        );
                    }
                    CalTrigger::Absolute(date) => {
                        let alarm_date = date.as_start_with_resolver(&start.timezone(), &resolver);
                        if resolved_in_range(alarm_date, start, end) {
                            let fstart = first
                                .start()
                                .map(|d| d.as_start_with_resolver(&start.timezone(), &resolver));
                            let fend = first
                                .end_or_due()
                                .map(|d| d.as_end_with_resolver(&start.timezone(), &resolver));
                            alarms.push(AlarmOccurrence::new(
                                Occurrence::new_in_tz(
                                    self.dir.clone(),
                                    first,
                                    fstart,
                                    fend,
                                    false,
                                    start.timezone(),
                                ),
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
                let rid_tz = rid.as_start_with_resolver(&start.timezone(), &resolver);
                if let Some(alarm) = alarms
                    .iter_mut()
                    .find(|a| a.occurrence().resolved_occurrence_start() == Some(rid_tz))
                {
                    alarm
                        .occurrence_mut()
                        .set_overwrite(overwrite, &start.timezone(), &resolver);
                }

                if let Some(alarms) = overwrite.alarms() {
                    alarm_overwrites.insert(rid, alarms);
                }
            }

            // let the overlay customize these overwrites
            let alarm_overwrites = overlay.alarm_overwrites(first, alarm_overwrites);

            for (rid, rid_alarms) in alarm_overwrites {
                // construct a new occurrence
                let rid_tz = rid.as_start_with_resolver(&start.timezone(), &resolver);
                let fend = first.time_duration().map(|d| rid_tz + d);
                let mut rid_occ = Occurrence::new_in_tz(
                    self.dir.clone(),
                    first,
                    Some(rid_tz),
                    fend,
                    false,
                    start.timezone(),
                );
                if let Some(overwrite) =
                    self.cal.components().iter().find(|c| c.rid() == Some(&rid))
                {
                    rid_occ.set_overwrite(overwrite, &start.timezone(), &resolver);
                }

                // remove all alarms we already had for this occurrence
                alarms.retain(|a| a.occurrence().resolved_occurrence_start() != Some(rid_tz));

                // add the desired ones (if they are in the specified time frame)
                for rid_alarm in rid_alarms {
                    let trigger_date = rid_alarm.trigger_date(
                        rid_occ.resolved_occurrence_start(),
                        rid_occ.resolved_occurrence_end(),
                        rid_occ.tz_offset(),
                    );
                    match trigger_date {
                        Some(alarm) if resolved_in_range(alarm, start, end) => {
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
        let resolver = self.cal.timezone_resolver();
        let first = self.component_with(|c| c.rid().is_none() && c.uid() == uid.as_ref())?;
        let (fstart, fend, excluded) = match rid {
            Some(rid) => (
                Some(rid.as_start_with_resolver(tz, &resolver)),
                None,
                first.exdates().contains(rid),
            ),
            None => (
                first
                    .start()
                    .map(|d| d.as_start_with_resolver(tz, &resolver)),
                first
                    .end_or_due()
                    .map(|d| d.as_end_with_resolver(tz, &resolver)),
                false,
            ),
        };
        let mut res = Occurrence::new_in_tz(self.dir.clone(), first, fstart, fend, excluded, *tz);

        if let Some(rid) = rid {
            let occ = self
                .cal
                .components()
                .iter()
                .find(|c| c.uid() == uid.as_ref() && c.rid() == Some(rid));
            if let Some(occ) = occ {
                res.set_overwrite(occ, tz, &resolver);
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
    pub fn occurrences_between<'s, F>(
        &'s self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
        filter: F,
    ) -> OccurrenceIterator<'s, 's>
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
            Some((
                first,
                first.dates_between(start, end, self.cal.timezone_resolver()),
            )),
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
                    let addr = a.address();
                    let cur_name = contacts.get_mut(&addr);
                    match cur_name {
                        Some(cur_name) if &addr == cur_name && a.common_name().is_some() => {
                            *cur_name = a.common_name().unwrap().clone();
                        }
                        None => {
                            let name = a.common_name().cloned().unwrap_or(addr);
                            contacts.insert(a.address().to_string(), name);
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
        self.cal.delete_components(|c| c.uid() == uid.as_ref());
    }

    /// Creates a new overwrite for the occurrence of the component with given uid at given date.
    ///
    /// The `uid` specifies the id of the base component, whereas the `rid` specifies the date of
    /// the occurrence in UTC. The timezone will be used to for the start date of the occurrence.
    /// The function `func` will be called with a reference to the base component and a mutable
    /// reference to the created overwrite, so that changes can be made before it is stored.
    ///
    /// Expects that the component with given uid exists, but *not* the overwrite.
    ///
    /// Returns `Err(ColError::ComponentNotFound)` if no base component with `uid` exists, and
    /// `Err(ColError::RidExists)` if an overwrite for `rid` is already present. Both errors are
    /// converted via `E::from`. Any error returned by `func` is propagated as-is.
    ///
    /// Note that this does not save to file. Please call [`Self::save`] to do so.
    pub fn create_overwrite<F, U, E>(
        &mut self,
        uid: U,
        rid: CalDate,
        tz: &Tz,
        func: F,
    ) -> Result<(), E>
    where
        F: FnOnce(&CalComponent, &mut CalComponent) -> Result<(), E>,
        U: ToString,
        E: From<ColError>,
    {
        let uid = uid.to_string();
        let base = self
            .components()
            .iter()
            .find(|c| c.uid() == &uid && c.rid().is_none())
            .ok_or_else(|| E::from(ColError::ComponentNotFound(uid.clone())))?;

        // does the overwrite exist?
        if self
            .components()
            .iter()
            .any(|c| c.uid() == &uid && c.rid() == Some(&rid))
        {
            return Err(E::from(ColError::RidExists(rid)));
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
        comp.set_rid(Some(rid.clone()));
        comp.set_last_modified(CalDate::now());
        comp.set_stamp(CalDate::now());

        func(base, &mut comp)?;

        info!("{}: creating overwrite for {} @ {}", self.dir, uid, rid);

        self.add_component(comp);
        Ok(())
    }

    /// Changes the start (and optionally the end or due date) of the base component with the
    /// given uid, and shifts all overwrite RECURRENCE-IDs by the same time delta.
    ///
    /// The method computes the delta between the old and new DTSTART as a UTC duration and applies
    /// it to the `RECURRENCE-ID` of every overwrite that belongs to the same uid. Overwrite
    /// DTSTARTs are intentionally left untouched so that any custom time a user placed an
    /// individual occurrence at is preserved in absolute terms.
    ///
    /// All new dates are validated against `local_tz` before any mutation takes place. If any
    /// date lands in a DST gap or DST fold the method returns `Err(ColError::Validation(...))`
    /// and the file is left unchanged.
    ///
    /// Returns `Err(ColError::ComponentNotFound)` if no base component with `uid` exists.
    ///
    /// Note that this does not save to file. Please call [`Self::save`] to do so.
    pub fn change_start(
        &mut self,
        uid: &str,
        new_start: CalDate,
        new_end_or_due: Option<CalDate>,
        local_tz: &Tz,
    ) -> Result<(), ColError> {
        // extract old start
        let old_start = self
            .components()
            .iter()
            .find(|c| c.uid() == uid && c.rid().is_none())
            .and_then(|c| c.start().cloned())
            .ok_or_else(|| ColError::ComponentNotFound(uid.to_string()))?;

        // Validate the new base dates before touching anything.
        new_start.validate(local_tz)?;
        if let Some(ref e) = new_end_or_due {
            e.validate(local_tz)?;
        }

        // Compute the UTC delta between old and new DTSTART.
        let delta: Duration = new_start.as_start_with_tz(&chrono_tz::UTC)
            - old_start.as_start_with_tz(&chrono_tz::UTC);

        // Collect per-overwrite update info so we can abort before any mutation if any
        // shifted RID (or DTSTART/end) falls in a DST gap or fold.
        //
        // An overwrite whose DTSTART equals its RID has only non-time fields customised (e.g.
        // summary), so we also shift its DTSTART and end by the same delta.  An overwrite whose
        // DTSTART differs from its RID was explicitly placed at a custom time by the user; in that
        // case only the RID is shifted and the absolute DTSTART is left unchanged.
        struct OverwriteUpdate {
            index: usize,
            new_rid: CalDate,
            /// New DTSTART to apply, or `None` to leave it unchanged.
            new_start: Option<CalDate>,
            /// New end/due to apply, or `None` to leave it unchanged.
            new_end: Option<CalDate>,
        }

        let overwrite_updates: Vec<OverwriteUpdate> = self
            .cal
            .components()
            .iter()
            .enumerate()
            .filter(|(_, c)| c.uid() == uid && c.rid().is_some())
            .map(|(i, c)| {
                let rid = c.rid().unwrap();
                // Convert the RID to match the variant of the new series start, shifting its
                // date by `delta`. This handles all-day ↔ timed conversions as well as
                // same-type shifts.
                let new_rid = Self::convert_rid(rid, delta, &new_start);
                new_rid.validate(local_tz)?;

                // Shift DTSTART only when it currently matches the RID (no custom time set).
                let (new_ow_start, new_ow_end) = if c.start() == Some(rid) {
                    let shifted_start = Self::convert_rid(rid, delta, &new_start);
                    shifted_start.validate(local_tz)?;
                    let shifted_end = c.end_or_due().map(|e| {
                        Self::convert_rid(e, delta, new_end_or_due.as_ref().unwrap_or(&new_start))
                    });
                    (Some(shifted_start), shifted_end)
                } else {
                    (None, None)
                };

                Ok(OverwriteUpdate {
                    index: i,
                    new_rid,
                    new_start: new_ow_start,
                    new_end: new_ow_end,
                })
            })
            .collect::<Result<_, ColError>>()?;

        // mutations start here
        info!("{}: changing start of {} by {:?}", self.dir, uid, delta);

        // Update overwrite RIDs (and optionally DTSTART/end).
        let now = CalDate::now();
        for upd in overwrite_updates {
            let comp = &mut self.cal.components_mut()[upd.index];
            comp.set_rid(Some(upd.new_rid));
            if let Some(s) = upd.new_start {
                comp.set_start(Some(s));
            }
            match (comp.ctype(), upd.new_end) {
                (CalCompType::Event, Some(e)) => comp.set_end_checked(Some(e), local_tz).unwrap(),
                (CalCompType::Todo, Some(d)) => comp.set_due_checked(Some(d), local_tz).unwrap(),
                _ => {}
            }
            comp.set_last_modified(now.clone());
            comp.set_stamp(now.clone());
        }

        // Update the base component.
        let base = self
            .cal
            .components_mut()
            .iter_mut()
            .find(|c| c.uid() == uid && c.rid().is_none())
            .unwrap(); // we already confirmed it exists above

        // we validated the dates above and have already changed the state
        base.set_start_checked(Some(new_start), local_tz).unwrap();
        match (base.ctype(), new_end_or_due) {
            (CalCompType::Event, Some(end)) => base.set_end_checked(Some(end), local_tz).unwrap(),
            (CalCompType::Todo, Some(due)) => base.set_due_checked(Some(due), local_tz).unwrap(),
            _ => {}
        }
        base.set_last_modified(now.clone());
        base.set_stamp(now);

        Ok(())
    }

    /// Shifts a [`CalDate`] by the given duration, preserving the date/datetime variant.
    ///
    /// For `Date` variants the day is shifted by the number of whole days in `delta`. For
    /// `DateTime` variants the naive wall-clock time is shifted directly so that DST transitions
    /// are handled the same way as in [`crate::objects::CalRRule`].
    fn shift_caldate(date: &CalDate, delta: Duration) -> CalDate {
        match date {
            CalDate::Date(d, ty) => {
                // Shift by whole days; fractional days from a time-only move are ignored for
                // all-day RIDs because the RID is always at midnight.
                let shifted = *d + delta;
                CalDate::Date(shifted, *ty)
            }
            CalDate::DateTime(CalDateTime::Timezone(naive, tz_name)) => {
                CalDate::DateTime(CalDateTime::Timezone(*naive + delta, tz_name.clone()))
            }
            CalDate::DateTime(CalDateTime::Floating(naive)) => {
                CalDate::DateTime(CalDateTime::Floating(*naive + delta))
            }
            CalDate::DateTime(CalDateTime::Utc(dt)) => {
                CalDate::DateTime(CalDateTime::Utc(*dt + delta))
            }
        }
    }

    /// Converts a RECURRENCE-ID (or DTSTART/end of an overwrite) to match the variant of
    /// `target_shape`, shifting the date by `delta` whole days in the process.
    ///
    /// When the series changes between all-day and timed (or vice versa), the RID must change its
    /// [`CalDate`] variant to match the new series start. The date is advanced by `delta` whole
    /// days (fractional days are ignored for all-day dates). For timed variants the time-of-day is
    /// taken from `target_shape` so that the RID points at the correct occurrence slot after the
    /// type change.
    fn convert_rid(date: &CalDate, delta: Duration, target_shape: &CalDate) -> CalDate {
        // Shift the source date within its own variant first to get the target NaiveDate.
        let shifted = Self::shift_caldate(date, delta);
        let shifted_naive_date = shifted.as_naive_date();

        match target_shape {
            // Target is all-day: produce a Date with the shifted day, preserving CalDateType.
            CalDate::Date(_, ty) => CalDate::Date(shifted_naive_date, *ty),
            // Target is timed: produce a DateTime whose date is the shifted day and whose
            // time-of-day matches the target shape.
            CalDate::DateTime(CalDateTime::Timezone(target_naive, tz_name)) => {
                let new_naive = shifted_naive_date.and_time(target_naive.time());
                CalDate::DateTime(CalDateTime::Timezone(new_naive, tz_name.clone()))
            }
            CalDate::DateTime(CalDateTime::Floating(target_naive)) => {
                let new_naive = shifted_naive_date.and_time(target_naive.time());
                CalDate::DateTime(CalDateTime::Floating(new_naive))
            }
            CalDate::DateTime(CalDateTime::Utc(target_dt)) => {
                let new_naive = shifted_naive_date.and_time(target_dt.naive_utc().time());
                CalDate::DateTime(CalDateTime::Utc(new_naive.and_utc()))
            }
        }
    }

    ///
    /// After parsing, all component dates are validated against `local_tz`. Components with times
    /// falling in a DST gap (non-existent) or DST fold (ambiguous) are removed with a warning.
    pub fn reload_calendar(&mut self, local_tz: &Tz) -> Result<(), ColError> {
        let cal = Self::read_calendar(&self.path, local_tz)?;
        self.cal = cal;
        Ok(())
    }

    fn read_calendar(path: &Path, local_tz: &Tz) -> Result<Calendar, ColError> {
        let mut input = String::new();
        File::open(path)
            .map_err(|e| ColError::FileOpen(path.to_path_buf(), e))?
            .read_to_string(&mut input)
            .map_err(|e| ColError::FileRead(path.to_path_buf(), e))?;

        let mut cal = input
            .parse::<Calendar>()
            .map_err(|e| ColError::FileParse(path.to_path_buf(), e))?;
        cal.validate_times(local_tz);
        Ok(cal)
    }

    /// Saves the current state to file.
    pub fn save(&self) -> Result<(), ColError> {
        info!("{}: writing file {:?}", self.dir, self.path);
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
        info!("{}: deleting file {:?}", self.dir, self.path);
        fs::remove_file(&self.path).map_err(|e| ColError::FileRemove(self.path.clone(), e))
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeDelta, TimeZone};

    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::col::{CalDir, ColError};
    use crate::objects::{
        CalAction, CalAlarm, CalAttendee, CalComponent, CalDate, CalDateTime, CalRRule, CalRelated,
        CalTrigger, DefaultAlarmOverlay, UpdatableEventLike,
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
            if !result.iter().any(|o| o.uid() == *uid) {
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
                .with_timezone(tz)
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
                .with_timezone(tz)
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
                        duration: (-Duration::days(2)).into(),
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
                        duration: Duration::days(1).into(),
                    },
                ))
                .done(),
        ));

        let overlay = DefaultAlarmOverlay;
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
                        duration: Duration::minutes(-10).into(),
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
                        duration: (-Duration::days(1)).into(),
                    },
                ))
                .done(),
        ));

        let overlay = DefaultAlarmOverlay;
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
                        duration: Duration::minutes(-10).into(),
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
                        duration: Duration::hours(1).into(),
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
                        duration: Duration::days(1).into(),
                    },
                ))
                .done(),
        ));
        dir.add_file(CalFile::new_simple(cal));

        let overlay = DefaultAlarmOverlay;
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
                        duration: TimeDelta::hours(1).into(),
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
                                    duration: (-TimeDelta::days(1)).into(),
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

    // -----------------------------------------------------------------------
    // CalFile accessors and mutators
    // -----------------------------------------------------------------------

    #[test]
    fn partial_eq_by_calendar() {
        // Two CalFiles wrapping the same Calendar are equal regardless of dir/path.
        let ics = concat!(
            "BEGIN:VCALENDAR\r\n",
            "VERSION:2.0\r\n",
            "PRODID:-//Test//Test//EN\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:uid-1\r\n",
            "DTSTART;VALUE=DATE:20240101\r\n",
            "DTEND;VALUE=DATE:20240102\r\n",
            "DTSTAMP:20240101T000000Z\r\n",
            "END:VEVENT\r\n",
            "END:VCALENDAR\r\n",
        );

        let file_a = CalFile::new(
            Arc::new("dir-a".into()),
            PathBuf::from("/a"),
            ics.parse::<Calendar>().unwrap(),
        );
        let file_b = CalFile::new(
            Arc::new("dir-b".into()),
            PathBuf::from("/b"),
            ics.parse::<Calendar>().unwrap(),
        );
        // Same calendar contents → equal, dir/path are ignored.
        assert_eq!(file_a, file_b);

        // A different calendar → not equal.
        let ics_other = concat!(
            "BEGIN:VCALENDAR\r\n",
            "VERSION:2.0\r\n",
            "PRODID:-//Test//Test//EN\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:uid-different\r\n",
            "DTSTART;VALUE=DATE:20240101\r\n",
            "DTEND;VALUE=DATE:20240102\r\n",
            "DTSTAMP:20240101T000000Z\r\n",
            "END:VEVENT\r\n",
            "END:VCALENDAR\r\n",
        );
        let file_c = CalFile::new_simple(ics_other.parse::<Calendar>().unwrap());
        assert_ne!(file_a, file_c);
    }

    #[test]
    fn directory_and_set_directory() {
        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(CalEvent::new("uid-1")));
        let initial_dir = Arc::new("initial".to_string());
        let mut file = CalFile::new(initial_dir.clone(), PathBuf::default(), cal);

        assert_eq!(file.directory(), &initial_dir);

        let new_dir = Arc::new("updated".to_string());
        file.set_directory(new_dir.clone());
        assert_eq!(file.directory(), &new_dir);
    }

    #[test]
    fn set_path_updates_path() {
        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(CalEvent::new("uid-1")));
        let mut file = CalFile::new(Arc::default(), PathBuf::from("/original"), cal);

        assert_eq!(file.path(), &PathBuf::from("/original"));

        file.set_path(PathBuf::from("/updated"));
        assert_eq!(file.path(), &PathBuf::from("/updated"));
    }

    #[test]
    fn calendar_and_calendar_mut() {
        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(CalEvent::new("uid-cal")));
        let mut file = CalFile::new_simple(cal);

        // calendar() gives read access.
        assert_eq!(file.calendar().components().len(), 1);

        // calendar_mut() allows adding a component.
        let mut extra = CalEvent::new("uid-extra");
        extra.set_start(Some(CalDate::Date(
            NaiveDate::from_ymd_opt(2024, 6, 1).unwrap(),
            CalCompType::Event.into(),
        )));
        file.calendar_mut()
            .add_component(CalComponent::Event(extra));
        assert_eq!(file.calendar().components().len(), 2);
    }

    #[test]
    fn add_component_appends() {
        let mut file = CalFile::new_simple(Calendar::default());
        assert!(file.components().is_empty());

        file.add_component(CalComponent::Event(CalEvent::new("uid-added")));
        assert_eq!(file.components().len(), 1);
        assert!(file.contains_uid("uid-added"));
    }

    #[test]
    fn events_and_todos_filter_correctly() {
        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(CalEvent::new("ev-uid")));
        cal.add_component(CalComponent::Todo(CalTodo::new("td-uid")));
        let file = CalFile::new_simple(cal);

        let events: Vec<_> = file.events().collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].uid(), "ev-uid");

        let todos: Vec<_> = file.todos().collect();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].uid(), "td-uid");
    }

    // -----------------------------------------------------------------------
    // contacts()
    // -----------------------------------------------------------------------

    fn make_attendee(address: &str, common_name: Option<&str>) -> CalAttendee {
        let mut a = CalAttendee::new(address.to_string());
        if let Some(cn) = common_name {
            a.set_common_name(cn.to_string());
        }
        a
    }

    fn make_event_with_attendees(uid: &str, attendees: Vec<CalAttendee>) -> CalFile {
        let mut event = CalEvent::new(uid);
        event.set_attendees(Some(attendees));
        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(event));
        CalFile::new_simple(cal)
    }

    #[test]
    fn contacts_empty_when_no_attendees() {
        let file = new_file(EventBuilder::new("uid").done());
        assert!(file.contacts().is_empty());
    }

    #[test]
    fn contacts_insert_address_without_common_name() {
        // When an attendee has no CN the address is stored as both key and value.
        let file = make_event_with_attendees("uid", vec![make_attendee("alice@example.com", None)]);
        let contacts = file.contacts();
        assert_eq!(
            contacts.get("alice@example.com"),
            Some(&"alice@example.com".to_string())
        );
    }

    #[test]
    fn contacts_insert_address_with_common_name() {
        // When an attendee has a CN the CN is stored as the value.
        let file = make_event_with_attendees(
            "uid",
            vec![make_attendee("bob@example.com", Some("Bob Smith"))],
        );
        let contacts = file.contacts();
        assert_eq!(
            contacts.get("bob@example.com"),
            Some(&"Bob Smith".to_string())
        );
    }

    #[test]
    fn contacts_upgrade_address_to_common_name() {
        // First encounter has no CN → address stored as value.  Second encounter for the same
        // address provides a CN → value is upgraded to the CN.
        let file = make_event_with_attendees(
            "uid",
            vec![
                make_attendee("carol@example.com", None),
                make_attendee("carol@example.com", Some("Carol White")),
            ],
        );
        let contacts = file.contacts();
        assert_eq!(
            contacts.get("carol@example.com"),
            Some(&"Carol White".to_string())
        );
    }

    #[test]
    fn contacts_no_change_when_already_has_name() {
        // When the stored value is already a real name (not the bare address), a subsequent
        // attendee entry for the same address without a CN must not overwrite the name.
        let file = make_event_with_attendees(
            "uid",
            vec![
                make_attendee("dave@example.com", Some("Dave Brown")),
                // Second entry: has CN but stored value is already "Dave Brown" (≠ address),
                // so the `_ => {}` arm fires and nothing changes.
                make_attendee("dave@example.com", Some("Dave B.")),
            ],
        );
        let contacts = file.contacts();
        // The first CN wins; subsequent different-CN entries hit the `_ => {}` branch.
        assert_eq!(
            contacts.get("dave@example.com"),
            Some(&"Dave Brown".to_string())
        );
    }

    // -----------------------------------------------------------------------
    // occurrence_by_id with RID
    // -----------------------------------------------------------------------

    #[test]
    fn occurrence_by_id_with_rid_no_overwrite() {
        // A recurring event without any overwrite component: querying by a specific RID
        // returns an occurrence that is not overwritten.
        let tz = &chrono_tz::Europe::Berlin;
        let base_date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let rid = CalDate::Date(base_date, CalCompType::Event.into());

        let file = new_file(
            EventBuilder::new("recurring")
                .start(rid.clone())
                .rrule("FREQ=DAILY;COUNT=5".parse().unwrap())
                .done(),
        );

        let occ = file.occurrence_by_id("recurring", Some(&rid), tz).unwrap();
        assert_eq!(occ.uid(), "recurring");
        assert!(!occ.is_overwritten());
    }

    #[test]
    fn occurrence_by_id_with_rid_and_overwrite() {
        // A recurring event where the second occurrence has an overwrite component: querying by
        // the second occurrence's RID returns an occurrence with the overwrite attached.
        let tz = &chrono_tz::Europe::Berlin;
        let base_date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let second_date = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        let rid = CalDate::Date(second_date, CalCompType::Event.into());

        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(
            EventBuilder::new("rec-ow")
                .start(CalDate::Date(base_date, CalCompType::Event.into()))
                .rrule("FREQ=DAILY;COUNT=3".parse().unwrap())
                .done(),
        ));
        // Overwrite component for the second occurrence. Set a custom summary to ensure the
        // overwrite's properties take precedence over the base component's values.
        let mut overwrite = EventBuilder::new("rec-ow")
            .start(CalDate::Date(second_date, CalCompType::Event.into()))
            .rid(rid.clone())
            .done();
        overwrite.set_summary(Some("Overwritten Summary".into()));
        cal.add_component(CalComponent::Event(overwrite));
        let file = CalFile::new_simple(cal);

        let occ = file.occurrence_by_id("rec-ow", Some(&rid), tz).unwrap();
        assert_eq!(occ.uid(), "rec-ow");
        assert!(occ.is_overwritten());
        // The occurrence should reflect the overwrite's summary, not the base component's.
        assert_eq!(
            occ.summary().map(|s| s.as_str()),
            Some("Overwritten Summary")
        );
    }

    #[test]
    fn occurrence_by_id_excluded_rid() {
        // When the queried RID is in the EXDATE list the occurrence is flagged as excluded.
        let tz = &chrono_tz::Europe::Berlin;
        let base_date = NaiveDate::from_ymd_opt(2024, 3, 1).unwrap();
        let excluded_date = NaiveDate::from_ymd_opt(2024, 3, 2).unwrap();
        let rid = CalDate::Date(excluded_date, CalCompType::Event.into());

        let file = new_file(
            EventBuilder::new("exc")
                .start(CalDate::Date(base_date, CalCompType::Event.into()))
                .rrule("FREQ=DAILY;COUNT=5".parse().unwrap())
                .exdate(rid.clone())
                .done(),
        );

        let occ = file.occurrence_by_id("exc", Some(&rid), tz).unwrap();
        assert!(occ.is_excluded());
    }

    // -----------------------------------------------------------------------
    // occurrences_between: no matching base component
    // -----------------------------------------------------------------------

    #[test]
    fn occurrences_between_no_matching_base() {
        // When the filter matches no component, the iterator must yield nothing.
        let file = new_allday_file(NaiveDate::from_ymd_opt(2024, 6, 1).unwrap(), "ev");
        let occs: Vec<_> = file
            .occurrences_between(new_date(2024, 6, 1), new_date(2024, 6, 30), |c| {
                c.uid() == "nonexistent"
            })
            .collect();
        assert!(occs.is_empty());
    }

    // -----------------------------------------------------------------------
    // create_overwrite
    // -----------------------------------------------------------------------

    #[test]
    fn create_overwrite_success_event() {
        // Happy path: create an overwrite for an existing recurring event.
        let tz = &chrono_tz::Europe::Berlin;
        let base_date = NaiveDate::from_ymd_opt(2024, 5, 1).unwrap();
        let rid = CalDate::Date(base_date, CalCompType::Event.into());

        let mut file = new_file(
            EventBuilder::new("ev-ow")
                .start(rid.clone())
                .rrule("FREQ=WEEKLY;COUNT=4".parse().unwrap())
                .done(),
        );

        file.create_overwrite::<_, _, ColError>("ev-ow", rid.clone(), tz, |_base, overwrite| {
            overwrite.set_summary(Some("Custom Summary".into()));
            Ok(())
        })
        .unwrap();

        // The overwrite component should now be present.
        let overwrite = file
            .component_with(|c| c.uid() == "ev-ow" && c.rid() == Some(&rid))
            .unwrap();
        assert_eq!(overwrite.summary(), Some(&"Custom Summary".to_string()));
    }

    #[test]
    fn create_overwrite_success_todo() {
        // Verifies that create_overwrite works for a VTODO base (the CalCompType::Todo branch).
        let tz = &chrono_tz::Europe::Berlin;
        let base_date = NaiveDate::from_ymd_opt(2024, 5, 1).unwrap();
        let rid = CalDate::Date(base_date, CalCompType::Todo.into());

        let mut todo = CalTodo::new("todo-ow");
        todo.set_start(Some(rid.clone()));
        todo.set_rrule(Some("FREQ=WEEKLY;COUNT=3".parse().unwrap()));

        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Todo(todo));
        let mut file = CalFile::new_simple(cal);

        file.create_overwrite::<_, _, ColError>("todo-ow", rid.clone(), tz, |_base, overwrite| {
            overwrite.set_summary(Some("Todo Overwrite".into()));
            Ok(())
        })
        .unwrap();

        let overwrite = file
            .component_with(|c| c.uid() == "todo-ow" && c.rid() == Some(&rid))
            .unwrap();
        assert_eq!(overwrite.ctype(), CalCompType::Todo);
        assert_eq!(overwrite.summary(), Some(&"Todo Overwrite".to_string()));
    }

    #[test]
    fn create_overwrite_uid_not_found() {
        let tz = &chrono_tz::Europe::Berlin;
        let rid = CalDate::Date(
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
            CalCompType::Event.into(),
        );
        let mut file = CalFile::new_simple(Calendar::default());

        let result: Result<(), ColError> =
            file.create_overwrite("no-such-uid", rid, tz, |_, _| Ok(()));
        assert!(matches!(result, Err(ColError::ComponentNotFound(_))));
    }

    #[test]
    fn create_overwrite_rid_already_exists() {
        // Creating a second overwrite for the same RID must fail.
        let tz = &chrono_tz::Europe::Berlin;
        let base_date = NaiveDate::from_ymd_opt(2024, 8, 5).unwrap();
        let rid = CalDate::Date(base_date, CalCompType::Event.into());

        let mut file = new_file(
            EventBuilder::new("dup-ow")
                .start(rid.clone())
                .rrule("FREQ=WEEKLY;COUNT=3".parse().unwrap())
                .done(),
        );

        // First overwrite: succeeds.
        file.create_overwrite::<_, _, ColError>("dup-ow", rid.clone(), tz, |_, _| Ok(()))
            .unwrap();

        // Second overwrite for the same RID: must fail.
        let result: Result<(), ColError> = file.create_overwrite("dup-ow", rid, tz, |_, _| Ok(()));
        assert!(matches!(result, Err(ColError::RidExists(_))));
    }

    // -----------------------------------------------------------------------
    // due_alarms_between edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn due_alarms_between_no_base_component() {
        // A CalFile that has only an overwrite component (rid is set) but no base component.
        // due_alarms_between must return an empty vec without panicking.
        let overwrite_date = NaiveDate::from_ymd_opt(2024, 1, 5).unwrap();
        let rid = CalDate::Date(overwrite_date, CalCompType::Event.into());
        let file = new_file(EventBuilder::new("ghost").rid(rid).done());

        let overlay = DefaultAlarmOverlay;
        let alarms = file.due_alarms_between(new_date(2024, 1, 1), new_date(2024, 1, 31), &overlay);
        assert!(alarms.is_empty());
    }

    // -----------------------------------------------------------------------
    // change_start
    // -----------------------------------------------------------------------

    #[test]
    fn change_start_uid_not_found() {
        // change_start must return ComponentNotFound when no base component with that uid exists.
        let tz = &chrono_tz::Europe::Berlin;
        let new_start = CalDate::Date(
            NaiveDate::from_ymd_opt(2024, 3, 1).unwrap(),
            CalCompType::Event.into(),
        );
        let mut file = CalFile::new_simple(Calendar::default());

        let result = file.change_start("no-such-uid", new_start, None, tz);
        assert!(matches!(result, Err(ColError::ComponentNotFound(_))));
    }

    #[test]
    fn change_start_non_recurrent() {
        // A simple (non-recurring) event: base start and end are updated, nothing else changes.
        let tz = &chrono_tz::Europe::Berlin;
        let old_start = NaiveDate::from_ymd_opt(2024, 6, 10).unwrap();
        let old_end = NaiveDate::from_ymd_opt(2024, 6, 11).unwrap();
        let new_start_date = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let new_end_date = NaiveDate::from_ymd_opt(2024, 6, 16).unwrap();

        let mut file = new_file(
            EventBuilder::new("single")
                .start(CalDate::Date(old_start, CalCompType::Event.into()))
                .end(CalDate::Date(old_end, CalCompType::Event.into()))
                .done(),
        );

        let new_start = CalDate::Date(new_start_date, CalCompType::Event.into());
        let new_end = CalDate::Date(new_end_date, CalCompType::Event.into());
        file.change_start("single", new_start.clone(), Some(new_end.clone()), tz)
            .unwrap();

        let base = file.component_with(|c| c.uid() == "single").unwrap();
        assert_eq!(base.start(), Some(&new_start));
        assert_eq!(base.end_or_due(), Some(&new_end));
        // No overwrites should exist.
        assert_eq!(file.components().len(), 1);
    }

    #[test]
    fn change_start_recurrent_no_overwrites() {
        // A recurring event with no overwrite components: only the base DTSTART and DTEND shift.
        let tz = &chrono_tz::Europe::Berlin;
        let start = NaiveDate::from_ymd_opt(2024, 1, 10)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let end = start + Duration::hours(1);
        let new_start_naive = NaiveDate::from_ymd_opt(2024, 1, 10)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap();
        let new_end_naive = new_start_naive + Duration::hours(1);

        let mut file = new_file(
            EventBuilder::new("recur")
                .start(CalDate::DateTime(CalDateTime::Timezone(
                    start,
                    "Europe/Berlin".to_string(),
                )))
                .end(CalDate::DateTime(CalDateTime::Timezone(
                    end,
                    "Europe/Berlin".to_string(),
                )))
                .rrule("FREQ=DAILY;COUNT=5".parse().unwrap())
                .done(),
        );

        let new_start = CalDate::DateTime(CalDateTime::Timezone(
            new_start_naive,
            "Europe/Berlin".to_string(),
        ));
        let new_end = CalDate::DateTime(CalDateTime::Timezone(
            new_end_naive,
            "Europe/Berlin".to_string(),
        ));
        file.change_start("recur", new_start.clone(), Some(new_end.clone()), tz)
            .unwrap();

        let base = file
            .component_with(|c| c.uid() == "recur" && c.rid().is_none())
            .unwrap();
        assert_eq!(base.start(), Some(&new_start));
        assert_eq!(base.end_or_due(), Some(&new_end));
        assert_eq!(file.components().len(), 1);
    }

    #[test]
    fn change_start_recurrent_with_overwrites() {
        // A recurring event where two occurrences have overwrite components. After change_start,
        // both overwrite RIDs must be shifted by the same delta.
        let tz = &chrono_tz::Europe::Berlin;

        // Base: daily at 09:00 for 5 days starting 2024-03-01.
        let base_start = NaiveDate::from_ymd_opt(2024, 3, 1)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let base_end = base_start + Duration::hours(1);

        // Overwrite for 2024-03-02 occurrence.
        let rid1_naive = NaiveDate::from_ymd_opt(2024, 3, 2)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let rid1 = CalDate::DateTime(CalDateTime::Timezone(
            rid1_naive,
            "Europe/Berlin".to_string(),
        ));

        // Overwrite for 2024-03-04 occurrence.
        let rid2_naive = NaiveDate::from_ymd_opt(2024, 3, 4)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let rid2 = CalDate::DateTime(CalDateTime::Timezone(
            rid2_naive,
            "Europe/Berlin".to_string(),
        ));

        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(
            EventBuilder::new("rec")
                .start(CalDate::DateTime(CalDateTime::Timezone(
                    base_start,
                    "Europe/Berlin".to_string(),
                )))
                .end(CalDate::DateTime(CalDateTime::Timezone(
                    base_end,
                    "Europe/Berlin".to_string(),
                )))
                .rrule("FREQ=DAILY;COUNT=5".parse().unwrap())
                .done(),
        ));
        // Overwrite 1: same time as base (no date shift), just a custom summary.
        let mut ow1 = EventBuilder::new("rec")
            .start(CalDate::DateTime(CalDateTime::Timezone(
                rid1_naive,
                "Europe/Berlin".to_string(),
            )))
            .end(CalDate::DateTime(CalDateTime::Timezone(
                rid1_naive + Duration::hours(1),
                "Europe/Berlin".to_string(),
            )))
            .rid(rid1.clone())
            .done();
        ow1.set_summary(Some("Custom One".into()));
        cal.add_component(CalComponent::Event(ow1));
        // Overwrite 2: same time as base, different summary.
        let mut ow2 = EventBuilder::new("rec")
            .start(CalDate::DateTime(CalDateTime::Timezone(
                rid2_naive,
                "Europe/Berlin".to_string(),
            )))
            .end(CalDate::DateTime(CalDateTime::Timezone(
                rid2_naive + Duration::hours(1),
                "Europe/Berlin".to_string(),
            )))
            .rid(rid2.clone())
            .done();
        ow2.set_summary(Some("Custom Two".into()));
        cal.add_component(CalComponent::Event(ow2));
        let mut file = CalFile::new_simple(cal);

        // Shift the series start by +1 hour (09:00 → 10:00).
        let new_base_start_naive = NaiveDate::from_ymd_opt(2024, 3, 1)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap();
        let new_base_end_naive = new_base_start_naive + Duration::hours(1);
        let new_start = CalDate::DateTime(CalDateTime::Timezone(
            new_base_start_naive,
            "Europe/Berlin".to_string(),
        ));
        let new_end = CalDate::DateTime(CalDateTime::Timezone(
            new_base_end_naive,
            "Europe/Berlin".to_string(),
        ));
        file.change_start("rec", new_start.clone(), Some(new_end.clone()), tz)
            .unwrap();

        // Base DTSTART must be updated.
        let base = file
            .component_with(|c| c.uid() == "rec" && c.rid().is_none())
            .unwrap();
        assert_eq!(base.start(), Some(&new_start));

        // Expected shifted RIDs (old + 1 hour).
        let expected_rid1 = CalDate::DateTime(CalDateTime::Timezone(
            rid1_naive + Duration::hours(1),
            "Europe/Berlin".to_string(),
        ));
        let expected_rid2 = CalDate::DateTime(CalDateTime::Timezone(
            rid2_naive + Duration::hours(1),
            "Europe/Berlin".to_string(),
        ));

        let ow1_comp = file
            .component_with(|c| c.uid() == "rec" && c.summary() == Some(&"Custom One".to_string()))
            .unwrap();
        assert_eq!(ow1_comp.rid(), Some(&expected_rid1));
        // Overwrite DTSTART == RID before the shift, so DTSTART is also shifted.
        assert_eq!(
            ow1_comp.start(),
            Some(&CalDate::DateTime(CalDateTime::Timezone(
                rid1_naive + Duration::hours(1),
                "Europe/Berlin".to_string(),
            )))
        );

        let ow2_comp = file
            .component_with(|c| c.uid() == "rec" && c.summary() == Some(&"Custom Two".to_string()))
            .unwrap();
        assert_eq!(ow2_comp.rid(), Some(&expected_rid2));
        // Same rule applies to the second overwrite.
        assert_eq!(
            ow2_comp.start(),
            Some(&CalDate::DateTime(CalDateTime::Timezone(
                rid2_naive + Duration::hours(1),
                "Europe/Berlin".to_string(),
            )))
        );
    }

    #[test]
    fn change_start_allday_series_with_overwrite() {
        // All-day series shifted by one day: the overwrite RID (a Date) advances by one day.
        let tz = &chrono_tz::Europe::Berlin;

        let base_date = NaiveDate::from_ymd_opt(2024, 5, 1).unwrap();
        let rid_date = NaiveDate::from_ymd_opt(2024, 5, 3).unwrap(); // 3rd occurrence

        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(
            new_allday_event(base_date, "allday")
                .rrule("FREQ=DAILY;COUNT=5".parse().unwrap())
                .done(),
        ));
        // Overwrite for the 3rd occurrence.
        let rid = CalDate::Date(rid_date, CalCompType::Event.into());
        let ow = EventBuilder::new("allday")
            .start(CalDate::Date(rid_date, CalCompType::Event.into()))
            .end(CalDate::Date(
                rid_date.succ_opt().unwrap(),
                CalCompType::Event.into(),
            ))
            .rid(rid.clone())
            .done();
        cal.add_component(CalComponent::Event(ow));
        let mut file = CalFile::new_simple(cal);

        // Shift the whole series by +1 day (May 1 → May 2).
        let new_base_date = NaiveDate::from_ymd_opt(2024, 5, 2).unwrap();
        let new_start = CalDate::Date(new_base_date, CalCompType::Event.into());
        let new_end = CalDate::Date(new_base_date.succ_opt().unwrap(), CalCompType::Event.into());
        file.change_start("allday", new_start.clone(), Some(new_end.clone()), tz)
            .unwrap();

        // Base must move.
        let base = file
            .component_with(|c| c.uid() == "allday" && c.rid().is_none())
            .unwrap();
        assert_eq!(base.start(), Some(&new_start));

        // Overwrite RID must advance by the same 1 day.
        let expected_rid = CalDate::Date(
            NaiveDate::from_ymd_opt(2024, 5, 4).unwrap(),
            CalCompType::Event.into(),
        );
        let ow_comp = file
            .component_with(|c| c.uid() == "allday" && c.rid().is_some())
            .unwrap();
        assert_eq!(ow_comp.rid(), Some(&expected_rid));
    }

    #[test]
    fn change_start_preserves_overwrite_absolute_time() {
        // When the series time changes, an overwrite that was placed at a custom absolute time
        // keeps its DTSTART unchanged. Only its RID is shifted to match the new series time.
        let tz = &chrono_tz::Europe::Berlin;

        // Series at 09:00, overwrite for 2nd occurrence manually placed at 14:00.
        let base_start = NaiveDate::from_ymd_opt(2024, 4, 1)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let rid_naive = NaiveDate::from_ymd_opt(2024, 4, 2)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let ow_custom_start = NaiveDate::from_ymd_opt(2024, 4, 2)
            .unwrap()
            .and_hms_opt(14, 0, 0)
            .unwrap(); // placed at 14:00 instead of 09:00

        let rid = CalDate::DateTime(CalDateTime::Timezone(
            rid_naive,
            "Europe/Berlin".to_string(),
        ));

        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(
            EventBuilder::new("pres")
                .start(CalDate::DateTime(CalDateTime::Timezone(
                    base_start,
                    "Europe/Berlin".to_string(),
                )))
                .end(CalDate::DateTime(CalDateTime::Timezone(
                    base_start + Duration::hours(1),
                    "Europe/Berlin".to_string(),
                )))
                .rrule("FREQ=DAILY;COUNT=3".parse().unwrap())
                .done(),
        ));
        cal.add_component(CalComponent::Event(
            EventBuilder::new("pres")
                .start(CalDate::DateTime(CalDateTime::Timezone(
                    ow_custom_start,
                    "Europe/Berlin".to_string(),
                )))
                .end(CalDate::DateTime(CalDateTime::Timezone(
                    ow_custom_start + Duration::hours(1),
                    "Europe/Berlin".to_string(),
                )))
                .rid(rid.clone())
                .done(),
        ));
        let mut file = CalFile::new_simple(cal);

        // Shift series from 09:00 to 10:00 (+1 hour).
        let new_base_start = NaiveDate::from_ymd_opt(2024, 4, 1)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap();
        file.change_start(
            "pres",
            CalDate::DateTime(CalDateTime::Timezone(
                new_base_start,
                "Europe/Berlin".to_string(),
            )),
            Some(CalDate::DateTime(CalDateTime::Timezone(
                new_base_start + Duration::hours(1),
                "Europe/Berlin".to_string(),
            ))),
            tz,
        )
        .unwrap();

        let ow_comp = file
            .component_with(|c| c.uid() == "pres" && c.rid().is_some())
            .unwrap();

        // RID shifted by +1 hour: was 09:00, now 10:00 on Apr 2.
        let expected_rid = CalDate::DateTime(CalDateTime::Timezone(
            rid_naive + Duration::hours(1),
            "Europe/Berlin".to_string(),
        ));
        assert_eq!(ow_comp.rid(), Some(&expected_rid));

        // Overwrite DTSTART is unchanged at 14:00 (absolute time preserved).
        let expected_ow_start = CalDate::DateTime(CalDateTime::Timezone(
            ow_custom_start,
            "Europe/Berlin".to_string(),
        ));
        assert_eq!(ow_comp.start(), Some(&expected_ow_start));
    }

    #[test]
    fn change_start_allday_to_timed_converts_rid() {
        // Converting an all-day series to a timed series: the overwrite RID must be converted
        // from CalDate::Date to CalDate::DateTime with the same date and the new start time.
        let tz = &chrono_tz::Europe::Berlin;

        let base_date = NaiveDate::from_ymd_opt(2024, 7, 1).unwrap();
        let rid_date = NaiveDate::from_ymd_opt(2024, 7, 3).unwrap(); // 3rd occurrence

        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(
            new_allday_event(base_date, "conv")
                .rrule("FREQ=DAILY;COUNT=5".parse().unwrap())
                .done(),
        ));
        // Overwrite for 3rd occurrence (all-day, DTSTART == RID).
        let rid = CalDate::Date(rid_date, CalCompType::Event.into());
        let ow = EventBuilder::new("conv")
            .start(CalDate::Date(rid_date, CalCompType::Event.into()))
            .end(CalDate::Date(
                rid_date.succ_opt().unwrap(),
                CalCompType::Event.into(),
            ))
            .rid(rid)
            .done();
        cal.add_component(CalComponent::Event(ow));
        let mut file = CalFile::new_simple(cal);

        // Convert series to timed: start at 10:00 on 2024-07-01.
        let new_base_naive = NaiveDate::from_ymd_opt(2024, 7, 1)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap();
        let new_start = CalDate::DateTime(CalDateTime::Timezone(
            new_base_naive,
            "Europe/Berlin".to_string(),
        ));
        let new_end = CalDate::DateTime(CalDateTime::Timezone(
            new_base_naive + Duration::hours(1),
            "Europe/Berlin".to_string(),
        ));
        file.change_start("conv", new_start, Some(new_end), tz)
            .unwrap();

        let ow_comp = file
            .component_with(|c| c.uid() == "conv" && c.rid().is_some())
            .unwrap();

        // RID must now be a DateTime on the same date as the old RID (Jul 3), at 10:00.
        let expected_rid = CalDate::DateTime(CalDateTime::Timezone(
            NaiveDate::from_ymd_opt(2024, 7, 3)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap(),
            "Europe/Berlin".to_string(),
        ));
        assert_eq!(ow_comp.rid(), Some(&expected_rid));

        // DTSTART was equal to the old RID (all-day, no custom time), so it is also converted.
        assert_eq!(ow_comp.start(), Some(&expected_rid));
    }

    #[test]
    fn change_start_timed_to_allday_converts_rid() {
        // Converting a timed series to an all-day series: the overwrite RID must be converted
        // from CalDate::DateTime to CalDate::Date, preserving the date.
        let tz = &chrono_tz::Europe::Berlin;

        let base_naive = NaiveDate::from_ymd_opt(2024, 8, 1)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let rid_naive = NaiveDate::from_ymd_opt(2024, 8, 3)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();

        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(
            EventBuilder::new("conv2")
                .start(CalDate::DateTime(CalDateTime::Timezone(
                    base_naive,
                    "Europe/Berlin".to_string(),
                )))
                .end(CalDate::DateTime(CalDateTime::Timezone(
                    base_naive + Duration::hours(1),
                    "Europe/Berlin".to_string(),
                )))
                .rrule("FREQ=DAILY;COUNT=5".parse().unwrap())
                .done(),
        ));
        // Overwrite for 3rd occurrence (DTSTART == RID, only summary changed).
        let rid = CalDate::DateTime(CalDateTime::Timezone(
            rid_naive,
            "Europe/Berlin".to_string(),
        ));
        let mut ow = EventBuilder::new("conv2")
            .start(rid.clone())
            .end(CalDate::DateTime(CalDateTime::Timezone(
                rid_naive + Duration::hours(1),
                "Europe/Berlin".to_string(),
            )))
            .rid(rid)
            .done();
        ow.set_summary(Some("Custom".into()));
        cal.add_component(CalComponent::Event(ow));
        let mut file = CalFile::new_simple(cal);

        // Convert series to all-day starting 2024-08-01.
        let new_base_date = NaiveDate::from_ymd_opt(2024, 8, 1).unwrap();
        let new_start = CalDate::Date(new_base_date, CalCompType::Event.into());
        let new_end = CalDate::Date(new_base_date.succ_opt().unwrap(), CalCompType::Event.into());
        file.change_start("conv2", new_start, Some(new_end), tz)
            .unwrap();

        let ow_comp = file
            .component_with(|c| c.uid() == "conv2" && c.rid().is_some())
            .unwrap();

        // RID must now be a Date on Aug 3 (same date as the old timed RID).
        let expected_rid = CalDate::Date(
            NaiveDate::from_ymd_opt(2024, 8, 3).unwrap(),
            CalCompType::Event.into(),
        );
        assert_eq!(ow_comp.rid(), Some(&expected_rid));

        // DTSTART was equal to the old RID, so it is also converted to all-day on Aug 3.
        assert_eq!(ow_comp.start(), Some(&expected_rid));
    }

    #[test]
    fn change_start_dst_gap_rejected() {
        // Trying to set the new start to a time that falls in a DST gap must be rejected.
        // In Europe/Berlin, 2025-03-30 02:30:00 does not exist (clocks jump from 02:00 to 03:00).
        let tz = &chrono_tz::Europe::Berlin;
        let base_start = NaiveDate::from_ymd_opt(2025, 3, 29)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();

        let mut file = new_file(
            EventBuilder::new("dst-gap")
                .start(CalDate::DateTime(CalDateTime::Timezone(
                    base_start,
                    "Europe/Berlin".to_string(),
                )))
                .end(CalDate::DateTime(CalDateTime::Timezone(
                    base_start + Duration::hours(1),
                    "Europe/Berlin".to_string(),
                )))
                .rrule("FREQ=DAILY;COUNT=3".parse().unwrap())
                .done(),
        );

        // 02:30 on 2025-03-30 is in the DST gap for Europe/Berlin.
        let gap_time = NaiveDate::from_ymd_opt(2025, 3, 30)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        let bad_start =
            CalDate::DateTime(CalDateTime::Timezone(gap_time, "Europe/Berlin".to_string()));

        let result = file.change_start("dst-gap", bad_start, None, tz);
        assert!(matches!(result, Err(ColError::Validation(_))));

        // File must be unchanged.
        let base = file
            .component_with(|c| c.uid() == "dst-gap" && c.rid().is_none())
            .unwrap();
        let original_start = CalDate::DateTime(CalDateTime::Timezone(
            base_start,
            "Europe/Berlin".to_string(),
        ));
        assert_eq!(base.start(), Some(&original_start));
    }

    #[test]
    fn change_start_overwrite_rid_dst_gap_rejected() {
        // When shifting the base start causes an overwrite RID to land in a DST gap, the whole
        // operation is rejected and the file is left unchanged.
        //
        // In Europe/Berlin, 2025-03-30 02:30:00 does not exist.
        // Series starts at 02:30 on 2025-03-29; shifting by +1 day would place the overwrite
        // RID at 02:30 on 2025-03-30 — which is in the gap.
        let tz = &chrono_tz::Europe::Berlin;

        // The 2nd occurrence RID is also at 02:30 on 2025-03-30 (which is in the DST gap).
        // We manufacture this by having the overwrite RID already be 02:30 on 2025-03-30.
        // But wait: that date is invalid, so we have to place the overwrite at a valid date
        // and then shift the series so the new RID lands in the gap.
        //
        // Strategy: series at 02:30 on 2025-03-28 (valid), overwrite for 2025-03-29 02:30
        // (valid). Shift series by +1 day → new RID would be 2025-03-30 02:30 (gap) → rejected.
        let series_start = NaiveDate::from_ymd_opt(2025, 3, 28)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        let rid_naive = NaiveDate::from_ymd_opt(2025, 3, 29)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        let rid = CalDate::DateTime(CalDateTime::Timezone(
            rid_naive,
            "Europe/Berlin".to_string(),
        ));

        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(
            EventBuilder::new("rid-gap")
                .start(CalDate::DateTime(CalDateTime::Timezone(
                    series_start,
                    "Europe/Berlin".to_string(),
                )))
                .end(CalDate::DateTime(CalDateTime::Timezone(
                    series_start + Duration::hours(1),
                    "Europe/Berlin".to_string(),
                )))
                .rrule("FREQ=DAILY;COUNT=3".parse().unwrap())
                .done(),
        ));
        cal.add_component(CalComponent::Event(
            EventBuilder::new("rid-gap")
                .start(CalDate::DateTime(CalDateTime::Timezone(
                    rid_naive,
                    "Europe/Berlin".to_string(),
                )))
                .end(CalDate::DateTime(CalDateTime::Timezone(
                    rid_naive + Duration::hours(1),
                    "Europe/Berlin".to_string(),
                )))
                .rid(rid.clone())
                .done(),
        ));
        let mut file = CalFile::new_simple(cal);

        // Shift series by +1 day: new base start = 2025-03-29 02:30 (valid),
        // but the existing overwrite RID would become 2025-03-30 02:30 (DST gap).
        let new_series_start = NaiveDate::from_ymd_opt(2025, 3, 29)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        let result = file.change_start(
            "rid-gap",
            CalDate::DateTime(CalDateTime::Timezone(
                new_series_start,
                "Europe/Berlin".to_string(),
            )),
            None,
            tz,
        );
        assert!(matches!(result, Err(ColError::Validation(_))));

        // File is unchanged: the original RID is still there.
        let ow = file
            .component_with(|c| c.uid() == "rid-gap" && c.rid().is_some())
            .unwrap();
        assert_eq!(ow.rid(), Some(&rid));

        // Base start is also unchanged.
        let base = file
            .component_with(|c| c.uid() == "rid-gap" && c.rid().is_none())
            .unwrap();
        assert_eq!(
            base.start(),
            Some(&CalDate::DateTime(CalDateTime::Timezone(
                series_start,
                "Europe/Berlin".to_string(),
            )))
        );
    }

    #[test]
    fn change_start_todo() {
        // change_start also works for VTODO components.
        let tz = &chrono_tz::Europe::Berlin;
        let start_date = NaiveDate::from_ymd_opt(2024, 7, 1).unwrap();
        let due_date = NaiveDate::from_ymd_opt(2024, 7, 2).unwrap();
        let new_start_date = NaiveDate::from_ymd_opt(2024, 7, 8).unwrap();
        let new_due_date = NaiveDate::from_ymd_opt(2024, 7, 9).unwrap();

        let mut todo = CalTodo::new("todo-cs");
        todo.set_start(Some(CalDate::Date(start_date, CalCompType::Todo.into())));
        todo.set_due(Some(CalDate::Date(due_date, CalCompType::Todo.into())));
        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Todo(todo));
        let mut file = CalFile::new_simple(cal);

        file.change_start(
            "todo-cs",
            CalDate::Date(new_start_date, CalCompType::Todo.into()),
            Some(CalDate::Date(new_due_date, CalCompType::Todo.into())),
            tz,
        )
        .unwrap();

        let base = file.component_with(|c| c.uid() == "todo-cs").unwrap();
        assert_eq!(
            base.start(),
            Some(&CalDate::Date(new_start_date, CalCompType::Todo.into()))
        );
        assert_eq!(
            base.end_or_due(),
            Some(&CalDate::Date(new_due_date, CalCompType::Todo.into()))
        );
    }
}
