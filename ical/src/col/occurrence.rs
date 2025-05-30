use std::sync::Arc;

use chrono::{DateTime, Duration, NaiveDate};
use chrono_tz::Tz;

use crate::objects::{
    CalAlarm, CalAttendee, CalCompType, CalComponent, CalDate, CalEventStatus, CalOrganizer,
    CalRRule, CalTodoStatus, CompDateType, EventLike,
};
use crate::parser::{Property, PropertyProducer};
use crate::util;

macro_rules! ctype_method {
    ($self:expr, $ctype:tt, $method:tt) => {
        match $self.overwrite {
            Some(o) if o.$ctype().and_then(|td| td.$method()).is_some() => {
                o.$ctype().and_then(|td| td.$method())
            }
            _ => $self.base.$ctype().and_then(|td| td.$method()),
        }
    };
}

/// An occurrence of an event/TODO.
///
/// If the event/TODO is non-recurrent, its occurrence is simply the single date on that it is
/// scheduled. Otherwise, the event/TODO has potentially many occurrences defined by the recurrence
/// rule and potential overwrites.
///
/// This struct contains both the base component and the overwritten component. When accessing
/// properties, the value of the base component will be delivered by default. If this property has
/// been overwritten, the value of the overwritten component will be delivered.
///
/// Occurrences do not necessarily have both a start and an end date. If the component they are
/// derived from do not have an end date, for example, the occurrence will not have one either.
/// However, recurrent components need to have a start date.
///
/// Occurrences can also be excluded. Most APIs will deliver these occurrences so that the caller
/// can decide whether to ignore them or not. The method [`Self::is_excluded`] can be used to check
/// it.
#[derive(Debug, Clone)]
pub struct Occurrence<'c> {
    dir: Arc<String>,
    start: Option<DateTime<Tz>>,
    end: Option<DateTime<Tz>>,
    base: &'c CalComponent,
    overwrite: Option<&'c CalComponent>,
    excluded: bool,
}

impl<'c> Occurrence<'c> {
    /// Creates a new occurrence at given start/end date.
    ///
    /// The `dir` specifies the directory the component lives in and `base` specifies the base
    /// component. `start` and `end` specify when this occurrence takes place. `excluded` specifies
    /// whether this occurrence has been excluded at the base component.
    pub fn new(
        dir: Arc<String>,
        base: &'c CalComponent,
        start: Option<DateTime<Tz>>,
        end: Option<DateTime<Tz>>,
        excluded: bool,
    ) -> Self {
        Self {
            dir,
            start,
            end,
            base,
            overwrite: None,
            excluded,
        }
    }

    /// Creates a new occurrence for a single date (either start or end).
    ///
    /// The `dir` specifies the directory the component lives in and `base` specifies the base
    /// component. `ty` specifies which date is present, whereas `date` specifies the date itself.
    /// `excluded` specifies whether this occurrence has been excluded at the base component.
    pub fn new_single(
        dir: Arc<String>,
        base: &'c CalComponent,
        ty: CompDateType,
        date: DateTime<Tz>,
        excluded: bool,
    ) -> Self {
        Self::new(
            dir,
            base,
            if ty == CompDateType::Start {
                Some(date)
            } else {
                None
            },
            if ty == CompDateType::EndOrDue {
                Some(date)
            } else {
                None
            },
            excluded,
        )
    }

    /// Returns the timezone used for this occurrence.
    ///
    /// Note that may be None in case neither the start or the end of the occurrence is known.
    pub fn tz(&self) -> Option<Tz> {
        match self.start {
            Some(start) => Some(start.timezone()),
            None => self.end.map(|end| end.timezone()),
        }
    }

    /// Returns the directory in which the underlying component lives.
    pub fn directory(&self) -> &Arc<String> {
        &self.dir
    }

    /// Returns the component type (event/TODO).
    pub fn ctype(&self) -> CalCompType {
        self.base.ctype()
    }

    /// Returns the base component.
    pub fn base(&self) -> &CalComponent {
        self.base
    }

    /// Returns true if the component has been overwritten.
    pub fn is_overwritten(&self) -> bool {
        self.overwrite.is_some()
    }

    /// Sets the given component as the overwrite for the contained base component.
    ///
    /// If an overwrite is set, its non-`None` properties will overwrite the properties of the base
    /// component.
    ///
    /// Note also that the start of this occurrence will be taken from the overwrite in case it has
    /// specified a start date.
    pub fn set_overwrite(&mut self, overwrite: &'c CalComponent) {
        self.overwrite = Some(overwrite);
        if let Some(ostart) = overwrite.start() {
            self.start = Some(ostart.as_start_with_tz(&self.tz().unwrap()));
        }
    }

    /// Returns true if this occurrence has been excluded.
    pub fn is_excluded(&self) -> bool {
        self.excluded
    }

    /// Returns true if this occurrence has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        match self.ctype() {
            CalCompType::Todo => {
                self.todo_status().unwrap_or(CalTodoStatus::InProcess) == CalTodoStatus::Cancelled
            }
            CalCompType::Event => {
                self.event_status().unwrap_or(CalEventStatus::Tentative)
                    == CalEventStatus::Cancelled
            }
        }
    }

    /// Returns the [`CalEventStatus`] in case this is a [`CalCompType::Event`].
    pub fn event_status(&self) -> Option<CalEventStatus> {
        ctype_method!(self, as_event, status)
    }

    /// Returns the [`CalTodoStatus`] in case this is a [`CalCompType::Todo`].
    pub fn todo_status(&self) -> Option<CalTodoStatus> {
        ctype_method!(self, as_todo, status)
    }

    /// Returns the percentage of completion in case this is a [`CalCompType::Todo`].
    pub fn todo_percent(&self) -> Option<u8> {
        ctype_method!(self, as_todo, percent)
    }

    /// Returns the completion date in case this is a [`CalCompType::Todo`].
    pub fn todo_completed(&self) -> Option<&CalDate> {
        ctype_method!(self, as_todo, completed)
    }

    /// Returns the start of this occurrence (if known).
    pub fn occurrence_start(&self) -> Option<DateTime<Tz>> {
        self.start
    }

    /// Returns the start of this occurrence (if known) as a [`CalDate`].
    pub fn occurrence_startdate(&self) -> Option<CalDate> {
        self.start.map(|start| {
            if self.is_all_day() {
                CalDate::Date(start.date_naive(), self.ctype().into())
            } else {
                start.into()
            }
        })
    }

    /// Returns the end of this occurrence (if known).
    pub fn occurrence_end(&self) -> Option<DateTime<Tz>> {
        match self.end {
            Some(end) => Some(end),
            None => self.duration().map(|d| self.start.unwrap() + d),
        }
    }

    /// Returns the end of this occurrence (if known) as a [`CalDate`].
    pub fn occurrence_enddate(&self) -> Option<CalDate> {
        self.occurrence_end().and_then(|e| {
            if self.is_all_day() {
                let date = match self.ctype() {
                    CalCompType::Todo => e.date_naive(),
                    CalCompType::Event => e.date_naive().succ_opt()?,
                };
                Some(CalDate::Date(date, self.ctype().into()))
            } else {
                Some(e.into())
            }
        })
    }

    /// Returns whether this occurrence starts on that given date.
    pub fn occurrence_starts_on(&self, date: NaiveDate) -> bool {
        match self.occurrence_start() {
            Some(start) => start.date_naive() == date,
            None => false,
        }
    }

    // Returns whether this occurrence ends on that given date.
    pub fn occurrence_ends_on(&self, date: NaiveDate) -> bool {
        self.occurrence_end()
            .map(|end| end.date_naive() == date)
            .unwrap_or(false)
    }

    /// Returns whether this occurrence lasts the complete day on the given date.
    pub fn is_all_day_on(&self, date: NaiveDate) -> bool {
        let end = self
            .occurrence_end()
            .map(|e| e.date_naive())
            .unwrap_or(date);
        match self.occurrence_start() {
            Some(start) => date > start.date_naive() && date < end,
            None => false,
        }
    }

    /// Returns the duration of this occurrence.
    pub fn duration(&self) -> Option<Duration> {
        self.tz().and_then(|tz| self.base.duration(&tz))
    }

    /// Returns whether this occurrence overlaps with given time period.
    pub fn overlaps(&self, start: DateTime<Tz>, end: DateTime<Tz>) -> bool {
        if let Some(ostart) = self.start {
            util::date_ranges_overlap(ostart, self.occurrence_end().unwrap_or(ostart), start, end)
        } else if let Some(oend) = self.occurrence_end() {
            oend >= start && oend < end
        } else {
            false
        }
    }
}

macro_rules! occ_or_base {
    ($self:tt, $method:tt) => {
        match $self.overwrite {
            Some(overwrite) => overwrite.$method(),
            _ => $self.base.$method(),
        }
    };
}

macro_rules! occ_or_base_opt {
    ($self:tt, $method:tt) => {
        match $self.overwrite {
            Some(overwrite) if overwrite.$method().is_some() => overwrite.$method(),
            _ => $self.base.$method(),
        }
    };
}

impl PropertyProducer for Occurrence<'_> {
    fn to_props(&self) -> Vec<Property> {
        let props = vec![];
        props
    }
}

impl EventLike for Occurrence<'_> {
    fn uid(&self) -> &String {
        occ_or_base!(self, uid)
    }

    fn stamp(&self) -> &CalDate {
        occ_or_base!(self, stamp)
    }

    fn created(&self) -> Option<&CalDate> {
        occ_or_base_opt!(self, created)
    }

    fn last_modified(&self) -> Option<&CalDate> {
        occ_or_base_opt!(self, last_modified)
    }

    fn start(&self) -> Option<&CalDate> {
        occ_or_base_opt!(self, start)
    }

    fn end_or_due(&self) -> Option<&CalDate> {
        occ_or_base_opt!(self, end_or_due)
    }

    fn summary(&self) -> Option<&String> {
        occ_or_base_opt!(self, summary)
    }

    fn description(&self) -> Option<&String> {
        occ_or_base_opt!(self, description)
    }

    fn location(&self) -> Option<&String> {
        occ_or_base_opt!(self, location)
    }

    fn categories(&self) -> Option<&[String]> {
        occ_or_base_opt!(self, categories)
    }

    fn organizer(&self) -> Option<&CalOrganizer> {
        occ_or_base_opt!(self, organizer)
    }

    fn attendees(&self) -> Option<&[CalAttendee]> {
        occ_or_base_opt!(self, attendees)
    }

    fn exdates(&self) -> &[CalDate] {
        self.base.exdates()
    }

    fn alarms(&self) -> Option<&[CalAlarm]> {
        occ_or_base_opt!(self, alarms)
    }

    fn rrule(&self) -> Option<&CalRRule> {
        occ_or_base_opt!(self, rrule)
    }

    fn rid(&self) -> Option<&CalDate> {
        occ_or_base_opt!(self, rid)
    }

    fn priority(&self) -> Option<u8> {
        occ_or_base_opt!(self, priority)
    }
}

/// An occurrence with a due alarm.
#[derive(Debug)]
pub struct AlarmOccurrence<'o> {
    occ: Occurrence<'o>,
    alarm: CalAlarm,
}

impl<'o> AlarmOccurrence<'o> {
    /// Creates a new instance for the given alarm that is associated with the given occurrence.
    pub fn new(occ: Occurrence<'o>, alarm: CalAlarm) -> Self {
        Self { occ, alarm }
    }

    /// Returns a reference to the occurrence for which the alarm should trigger
    pub fn occurrence(&self) -> &Occurrence<'o> {
        &self.occ
    }

    pub(crate) fn occurrence_mut(&mut self) -> &mut Occurrence<'o> {
        &mut self.occ
    }

    /// Returns the alarm that should trigger
    pub fn alarm(&self) -> &CalAlarm {
        &self.alarm
    }

    /// Returns the first alarm date of this occurrence, if it has an alarm.
    pub fn alarm_date(&self) -> Option<DateTime<Tz>> {
        self.alarm
            .trigger_date(self.occ.occurrence_start(), self.occ.occurrence_end())
    }
}
