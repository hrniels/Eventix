use chrono::Duration;
use chrono_tz::Tz;

use crate::{
    objects::{CalAlarm, CalAttendee, CalDate, CalOrganizer, CalPartStat, CalRRule},
    parser::PropertyProducer,
};

use super::CalCompType;

/// Shared readable properties for events and TODOs.
///
/// This trait offers methods for reading these properties. [`UpdatableEventLike`] offers methods
/// for changing them.
pub trait EventLike: PropertyProducer {
    /// Returns the component type.
    fn ctype(&self) -> CalCompType;

    /// Returns the unique id (UID).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.4.7>.
    fn uid(&self) -> &String;

    /// Returns the date and time when this calendar object was revised last (DTSTAMP).
    ///
    /// The date is always specified in UTC.
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.7.2>.
    fn stamp(&self) -> &CalDate;

    /// Returns the date and time when this calendar object was created (CREATED).
    ///
    /// The date is always specified in UTC.
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.7.1>.
    fn created(&self) -> Option<&CalDate>;

    /// Returns the date and time when this calendar object was last changed (LAST_MODIFIED).
    ///
    /// The date is always specified in UTC.
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.7.3>.
    fn last_modified(&self) -> Option<&CalDate>;

    /// Returns the beginning of this calendar object (DTSTART).
    ///
    /// For recurrent objects, this property is mandatory, because it marks the beginning of the
    /// sequence of occurrences. Otherwise, it is optional.
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.2.4>.
    fn start(&self) -> Option<&CalDate>;

    /// Returns the end (or due date) of this calendar object.
    ///
    /// For events, this is always the end date (DTEND), whereas for TODOs, it's the due date
    /// (DUE).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.2.2> and
    /// <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.2.3>.
    fn end_or_due(&self) -> Option<&CalDate>;

    /// Returns true if this calendar object lasts for complete days.
    ///
    /// This requires either the start, end, or due date to be of type [`CalDate::Date`], so
    /// lacking the time specification.
    fn is_all_day(&self) -> bool {
        matches!(self.start(), Some(CalDate::Date(..)))
            || matches!(self.end_or_due(), Some(CalDate::Date(..)))
    }

    /// Calculates the duration of this calendar object.
    ///
    /// The calculation is based on [`Self::start`] and [`Self::end_or_due`] and yields `None` if
    /// either is `None`.
    fn duration(&self) -> Option<Duration> {
        let start = self.start()?;

        // ensure that we start day-aligned if either start or end is all-day
        let start = if self.is_all_day() && !matches!(start, CalDate::Date(..)) {
            CalDate::Date(start.as_naive_date(), self.ctype().into())
        } else {
            start.clone()
        };

        let tz = Tz::UTC;
        self.end_or_due()
            .map(|end| end.as_end_with_tz(&tz) - start.as_start_with_tz(&tz))
    }

    /// Returns the summary of the calendar object (SUMMARY).
    ///
    /// The summary is a short version of the [`Self::description`].
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.1.12>.
    fn summary(&self) -> Option<&String>;

    /// Returns the description of the calendar object (DESCRIPTION).
    ///
    /// The description is the long version of the [`Self::summary`].
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.1.5>.
    fn description(&self) -> Option<&String>;

    /// Returns the location of the calendar object (LOCATION).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.1.7>.
    fn location(&self) -> Option<&String>;

    /// Returns a slice of categories the calendar object belongs to (CATEGORIES).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.1.2>.
    fn categories(&self) -> Option<&[String]>;

    /// Returns the organizer of this calendar object (ORGANIZER).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.4.3>.
    fn organizer(&self) -> Option<&CalOrganizer>;

    /// Returns true if this calendar object is owned by the given user.
    ///
    /// The user is expected to be an email address that is compared to the organizer's email
    /// address.
    fn is_owned_by<S: AsRef<str>>(&self, user: Option<S>) -> bool {
        match (self.organizer(), user) {
            (Some(ev_org), Some(user))
                if ev_org.address().to_lowercase() == user.as_ref().to_lowercase() =>
            {
                true
            }
            (Some(_), _) => false,
            (None, _) => true,
        }
    }

    /// Returns a slice of attendees for this calendar object (ATTENDEE).
    ///
    /// Note that attendees are an `Option` of a slice, because not having specified attendees
    /// (like with other properties) for overwritten calendar objects should mean that we take the
    /// attendees from the base calendar object. Whereas `Some(vec[])` means that the attendee list
    /// for this specific overwritten calendar object is empty.
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.4.1>.
    fn attendees(&self) -> Option<&[CalAttendee]>;

    /// Returns the status of the attendee with given email address.
    ///
    /// If there are no attendees, None is returned. Note also that not being in the attendee list
    /// or not having a status is considered as [`CalPartStat::NeedsAction`].
    fn attendee_status<M: AsRef<str>>(&self, user_mail: M) -> Option<CalPartStat> {
        self.attendees().map(|atts| {
            if let Some(att) = atts
                .iter()
                .find(|a| a.address().to_lowercase() == user_mail.as_ref().to_lowercase())
            {
                att.part_stat().unwrap_or(CalPartStat::NeedsAction)
            } else {
                // if the user is not part of the list (e.g., invited via mailing list), it's
                // considered as "needs action".
                CalPartStat::NeedsAction
            }
        })
    }

    /// Returns a slice of occurrence dates that are excluded (EXDATE).
    ///
    /// For recurrent calendar objects, this marks the occurrences that are not taking place. As
    /// excluded dates cannot be specified for overwritten calendar objects, this property is not
    /// stored in an `Option` (see [`Self::attendees`] for more details).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.5.1>.
    fn exdates(&self) -> &[CalDate];

    /// Returns a slice of alarms that are set for this calendar object (VALARM).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.6.6>.
    fn alarms(&self) -> Option<&[CalAlarm]>;

    /// Returns true if this calendar object has alarms.
    fn has_alarms(&self) -> bool {
        self.alarms().map(|a| !a.is_empty()).unwrap_or(false)
    }

    /// Returns the recurrence rule for this calendar object (RRULE).
    ///
    /// If this calendar object is recurrent, this method returns `Some` with a reference to the
    /// [`CalRRule`] that describes the pattern of recurrence. For example, a calendar object might
    /// repeat every 4 weeks and on each last tuesday of the month.
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.5.3>.
    fn rrule(&self) -> Option<&CalRRule>;

    /// Returns the recurrence id (RECURRENCE-ID).
    ///
    /// The recurrence id is `Some` if this calendar object is not the ``base'' component, but an
    /// overwritten component. For example, the overwrite might change the summary, date, or
    /// location, or cancel this specific occurrence.
    ///
    /// Note that a calendar object that has a RECURRENCE-ID has no RRULE and vice versa.
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.4.4>.
    fn rid(&self) -> Option<&CalDate>;

    /// Returns true if this calendar object is recurrent.
    fn is_recurrent(&self) -> bool {
        self.rrule().is_some() || self.rid().is_some()
    }

    /// Returns the priority (PRIORITY).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.1.9>.
    fn priority(&self) -> Option<u8>;
}

/// Shared changable properties for events and TODOs.
///
/// This trait offers methods for changing these properties. [`EventLike`] offers methods for
/// reading them.
pub trait UpdatableEventLike: EventLike {
    /// Sets the start of this calendar object to given value.
    fn set_start(&mut self, start: Option<CalDate>);

    /// Sets the summary of this calendar object to given value.
    fn set_summary(&mut self, summary: Option<String>);

    /// Sets the location of this calendar object to given value.
    fn set_location(&mut self, location: Option<String>);

    /// Sets the description of this calendar object to given value.
    fn set_description(&mut self, desc: Option<String>);

    /// Sets the date of the last modification.
    fn set_last_modified(&mut self, date: CalDate);

    /// Sets the date of the last modification.
    fn set_stamp(&mut self, date: CalDate);

    /// Sets the recurrence rule.
    ///
    /// Note that the recurrence rule being `Some` requires the recurrence id to be `None`.
    fn set_rrule(&mut self, rrule: Option<CalRRule>);

    /// Sets the recurrence id.
    ///
    /// Note that the recurrence rule being `Some` requires the recurrence id to be `None`.
    fn set_rid(&mut self, rid: Option<CalDate>);

    /// Sets the alarms to given vector of [`CalAlarm`].
    fn set_alarms(&mut self, alarms: Option<Vec<CalAlarm>>);

    /// Toggles the exclusion for given date.
    ///
    /// That is, if it is excluded, the exclusion will be removed. And otherwise, it will be added.
    fn toggle_exclude(&mut self, date: CalDate);

    /// Sets the attendees to given list.
    ///
    /// Note that attendees are an `Option` of a `Vec`, because not having specified attendees
    /// (like with other properties) for overwritten calendar objects should mean that we take the
    /// attendees from the base calendar object. Whereas `Some(vec[])` means that the attendee list
    /// for this specific overwritten calendar object is empty.
    fn set_attendees(&mut self, attendees: Option<Vec<CalAttendee>>);

    /// Sets the organizer.
    fn set_organizer(&mut self, organizer: Option<CalOrganizer>);

    /// Sets the priority.
    fn set_priority(&mut self, prio: Option<u8>);
}
