use std::{collections::HashMap, fmt::Display, io::BufRead, str::FromStr};

use serde::de::{Deserialize, Deserializer};
use serde::ser::{Serialize, Serializer};

use chrono::{DateTime, Duration};
use chrono_tz::Tz;

use crate::objects::CalDuration;
use crate::{
    objects::{CalComponent, CalDate, EventLike},
    parser::{LineReader, Parameter, ParseError, Property, PropertyConsumer, PropertyProducer},
};

/// The action for VALARM components.
///
/// Implements [`Display`] and [`FromStr`] to be turned into a string representation and vice
/// versa.
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.6.1>.
#[derive(Default, Copy, Clone, Debug, Eq, PartialEq)]
pub enum CalAction {
    /// Play a sound
    Audio,
    /// Display a text message
    #[default]
    Display,
    /// Send an email
    Email,
}

impl Display for CalAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Audio => write!(f, "AUDIO"),
            Self::Display => write!(f, "DISPLAY"),
            Self::Email => write!(f, "EMAIL"),
        }
    }
}

impl FromStr for CalAction {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "AUDIO" => Ok(Self::Audio),
            "DISPLAY" => Ok(Self::Display),
            "EMAIL" => Ok(Self::Email),
            _ => Err(ParseError::InvalidAction(s.to_string())),
        }
    }
}

/// The relation of alarms durations (start/end of event).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CalRelated {
    /// Relative to the start of the event.
    Start,
    /// Relative to the end of the event.
    End,
}

impl Display for CalRelated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Start => write!(f, "START"),
            Self::End => write!(f, "END"),
        }
    }
}

/// The trigger for iCalendar alarms.
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.6.3>.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CalTrigger {
    /// Fires at a time relative to the start/end of the event.
    Relative {
        related: CalRelated,
        duration: CalDuration,
    },
    /// Fires at an absolute time.
    Absolute(CalDate),
}

impl Default for CalTrigger {
    fn default() -> Self {
        Self::Relative {
            related: CalRelated::Start,
            duration: Duration::zero().into(),
        }
    }
}

impl CalTrigger {
    /// Turns this trigger into a [`Property`].
    pub fn to_prop(&self) -> Property {
        let mut params = Vec::new();
        let value = match self {
            Self::Relative { related, duration } => {
                params.push(Parameter::new("RELATED", format!("{related}")));
                duration.to_string()
            }
            Self::Absolute(date) => {
                let prop = date.to_prop("DUMMY");
                params.extend_from_slice(prop.params());
                prop.value().to_string()
            }
        };
        Property::new("TRIGGER", params, value)
    }
}

impl TryFrom<Property> for CalTrigger {
    type Error = ParseError;

    fn try_from(prop: Property) -> Result<Self, Self::Error> {
        if prop.value().starts_with("-P")
            || prop.value().starts_with("+P")
            || prop.value().starts_with("P")
        {
            let related = if prop.has_param_value("RELATED", "END") {
                CalRelated::End
            } else {
                CalRelated::Start
            };
            Ok(Self::Relative {
                related,
                duration: prop.value().parse()?,
            })
        } else {
            Ok(Self::Absolute(prop.try_into()?))
        }
    }
}

/// Represents an iCalendar alarm.
///
/// Such an alarm has a [`CalAction`] (e.g., display message) and a [`CalTrigger`] (e.g., trigger
/// 10minutes after the start of the event). Optionally, there are other properties such as a
/// description or a repetition.
///
/// Note that the [`Display`] implementation turns the object into a human readable description of
/// the alarm.
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.6.6>
#[derive(Clone, Default, Debug, Eq, PartialEq)]
pub struct CalAlarm {
    action: CalAction,
    trigger: CalTrigger,
    description: Option<String>,
    duration: Option<CalDuration>,
    repeat: Option<u8>,
    other: Vec<Property>,
}

impl CalAlarm {
    /// Creates a new alarm with given action and trigger.
    pub fn new(action: CalAction, trigger: CalTrigger) -> Self {
        Self {
            action,
            trigger,
            ..Default::default()
        }
    }

    /// Returns the action of the alarm.
    pub fn action(&self) -> CalAction {
        self.action
    }

    /// Returns the trigger of the alarm.
    pub fn trigger(&self) -> &CalTrigger {
        &self.trigger
    }

    /// Returns the duration.
    ///
    /// The duration specifies the delay between repeating alarms.
    pub fn duration(&self) -> Option<CalDuration> {
        self.duration
    }

    /// Returns the trigger date from the given start/end of an event.
    ///
    /// Assuming that the event starts at `start` and ends at `end`, this method returns the
    /// absolute time at which the alarm should trigger. Note that both start and end are optional,
    /// potentially leading to `None` being returned. That is, if the alarm is relative to the
    /// start and the start is `None`, the result will be `None` as well.
    pub fn trigger_date(
        &self,
        start: Option<DateTime<Tz>>,
        end: Option<DateTime<Tz>>,
        tz: Option<Tz>,
    ) -> Option<DateTime<Tz>> {
        match &self.trigger {
            CalTrigger::Relative { related, duration } => match related {
                CalRelated::Start => start.map(|s| s + **duration),
                CalRelated::End => end.map(|e| e + **duration),
            },
            CalTrigger::Absolute(date) => tz.map(|tz| date.as_start_with_tz(&tz)),
        }
    }

    /// Returns a human-readable representation of this description.
    pub fn human<'a, 't>(&'a self, tz: &'t Tz) -> AlarmHuman<'a, 't> {
        AlarmHuman { alarm: self, tz }
    }
}

/// Implements [`Display`](fmt::Display) to create a human-readable representation of a
/// [`CalAlarm`].
///
/// For example, it could say "3rd to last Wednesday".
pub struct AlarmHuman<'a, 't> {
    alarm: &'a CalAlarm,
    tz: &'t Tz,
}

impl std::fmt::Display for AlarmHuman<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.alarm.trigger {
            CalTrigger::Relative { related, duration } => {
                let (prefix, duration) = if **duration < Duration::zero() {
                    ("before", CalDuration::from(-**duration))
                } else {
                    ("after", CalDuration::from(**duration))
                };
                write!(
                    f,
                    "{} {} {}",
                    duration.human(),
                    prefix,
                    match related {
                        CalRelated::Start => "start",
                        CalRelated::End => "end",
                    }
                )
            }
            CalTrigger::Absolute(dt) => write!(f, "On {}", dt.fmt_start_with_tz(self.tz)),
        }
    }
}

impl Display for CalAlarm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for prop in self.to_props() {
            writeln!(f, "{prop}")?;
        }
        Ok(())
    }
}

impl FromStr for CalAlarm {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut lines = LineReader::new(s.as_bytes());
        lines.next().unwrap(); // skip BEGIN:VALARM
        CalAlarm::from_lines(&mut lines, Property::new("", vec![], ""))
    }
}

impl Serialize for CalAlarm {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{self}"))
    }
}

impl<'de> Deserialize<'de> for CalAlarm {
    fn deserialize<D>(deserializer: D) -> Result<CalAlarm, D::Error>
    where
        D: Deserializer<'de>,
    {
        let buf = String::deserialize(deserializer)?;
        buf.parse().map_err(serde::de::Error::custom)
    }
}

impl PropertyProducer for CalAlarm {
    fn to_props(&self) -> Vec<Property> {
        let mut props = Vec::new();
        props.push(Property::new("BEGIN", vec![], "VALARM"));
        props.push(Property::new("ACTION", vec![], format!("{}", self.action)));
        props.push(self.trigger.to_prop());
        if let Some(desc) = &self.description {
            props.push(Property::new("DESCRIPTION", vec![], desc.to_string()));
        }
        if let Some(duration) = &self.duration {
            props.push(Property::new("DURATION", vec![], duration.to_string()));
        }
        if let Some(repeat) = &self.repeat {
            props.push(Property::new("REPEAT", vec![], format!("{repeat}")));
        }
        props.extend(self.other.iter().cloned());
        props.push(Property::new("END", vec![], "VALARM"));
        props
    }
}

impl PropertyConsumer for CalAlarm {
    fn from_lines<R: BufRead>(
        lines: &mut LineReader<R>,
        _prop: Property,
    ) -> Result<Self, ParseError>
    where
        Self: Sized,
    {
        let mut comp = Self::default();
        loop {
            let Some(line) = lines.next() else {
                break Err(ParseError::UnexpectedEOF);
            };

            let prop = line.parse::<Property>()?;
            match prop.name().as_str() {
                "END" => {
                    if prop.value() != "VALARM" {
                        return Err(ParseError::UnexpectedEnd(prop.take_value()));
                    }
                    break Ok(comp);
                }
                "ACTION" => {
                    comp.action = prop.value().parse()?;
                }
                "TRIGGER" => {
                    comp.trigger = prop.try_into()?;
                }
                "DESCRIPTION" => {
                    comp.description = Some(prop.take_value());
                }
                "DURATION" => {
                    comp.duration = Some(prop.value().parse()?);
                }
                "REPEAT" => {
                    comp.repeat = Some(prop.value().parse()?);
                }
                _ => {
                    comp.other.push(prop);
                }
            }
        }
    }
}

/// A per-component and per-occurrence overlay for alarms.
///
/// This trait is used to gather due alarms within a specific time frame and allows to customize
/// the alarms on a per-component and per-occurrence basis.
pub trait AlarmOverlay {
    /// Returns the alarms for the given component.
    ///
    /// This method will be called for every component, regardless of whether it's recurrent or
    /// not. The component might have alarms set, but any list of alarms can be returned. Both
    /// `None` and `Some(vec![])` indicate no alarms in this case.
    fn alarms_for_component(&self, comp: &CalComponent) -> Option<Vec<CalAlarm>>;

    /// Returns the alarms that are overwritten for specific occurrences of the given component.
    ///
    /// This method will be called for every recurrent component. It receives the overwritten
    /// alarms for its occurrences and allows to customize these.
    ///
    /// This method returns a [`BTreeMap`] with the recurrence-id (CalDate in UTC) as the key and a
    /// [`Vec`] of [`CalAlarm`] as the vaues. If the map does not have an entry for a specific
    /// occurrence, the alarms from the base component will be taken. Otherwise the set alarms will
    /// be taken (which can be none).
    fn alarm_overwrites(
        &self,
        comp: &CalComponent,
        overwrites: HashMap<CalDate, &[CalAlarm]>,
    ) -> HashMap<CalDate, Vec<CalAlarm>>;
}

/// The default alarm overlay.
///
/// The default implementation simply takes the alarms from the calendar components.
#[derive(Default)]
pub struct DefaultAlarmOverlay;

impl AlarmOverlay for DefaultAlarmOverlay {
    fn alarms_for_component(&self, comp: &CalComponent) -> Option<Vec<CalAlarm>> {
        comp.alarms().map(|a| a.to_vec())
    }

    fn alarm_overwrites(
        &self,
        _comp: &CalComponent,
        overwrites: HashMap<CalDate, &[CalAlarm]>,
    ) -> HashMap<CalDate, Vec<CalAlarm>> {
        let mut res = HashMap::new();
        for (rid, alarms) in overwrites {
            res.insert(rid, alarms.to_vec());
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufWriter;

    use chrono::{Duration, TimeZone, Utc};

    use crate::{objects::CalDateTime, parser::LineWriter};

    use super::*;

    #[test]
    fn duration() {
        let dur = CalDuration::from_str("P15DT5H0M20S").unwrap();
        assert_eq!(dur.num_seconds(), 15 * 86400 + 5 * 3600 + 20);

        let dur = CalDuration::from_str("P1DT2H15M").unwrap();
        assert_eq!(dur.num_seconds(), 1 * 86400 + 2 * 3600 + 15 * 60);

        let dur = CalDuration::from_str("P1DT2H").unwrap();
        assert_eq!(dur.num_seconds(), 1 * 86400 + 2 * 3600);

        let dur = CalDuration::from_str("+P2W").unwrap();
        assert_eq!(dur.num_seconds(), 14 * 86400);

        let dur = CalDuration::from_str("-PT2H4M10S").unwrap();
        assert_eq!(dur.num_seconds(), -(2 * 3600 + 4 * 60 + 10));

        let dur = CalDuration::from_str("P10D").unwrap();
        assert_eq!(dur.num_seconds(), 10 * 86400);

        let dur = CalDuration::from_str("-P10DT4H").unwrap();
        assert_eq!(dur.num_seconds(), -(10 * 86400 + 4 * 3600));
    }

    #[test]
    fn duration_errors() {
        let dur = CalDuration::from_str("");
        assert!(matches!(dur, Err(ParseError::InvalidDuration(_))));

        let dur = CalDuration::from_str("P2");
        assert!(matches!(dur, Err(ParseError::InvalidDuration(_))));

        let dur = CalDuration::from_str("P2W1D");
        assert!(matches!(dur, Err(ParseError::InvalidDuration(_))));
    }

    #[test]
    fn trigger() {
        let prop: Property = "TRIGGER:-PT15M".parse().unwrap();
        let trigger: CalTrigger = prop.try_into().unwrap();
        match trigger {
            CalTrigger::Relative { related, duration } => {
                assert_eq!(related, CalRelated::Start);
                assert_eq!(duration, (-Duration::minutes(15)).into());
            }
            _ => panic!("expected CalTrigger::Relative"),
        }

        let prop: Property = "TRIGGER;RELATED=END:PT5M".parse().unwrap();
        let trigger: CalTrigger = prop.try_into().unwrap();
        match trigger {
            CalTrigger::Relative { related, duration } => {
                assert_eq!(related, CalRelated::End);
                assert_eq!(duration, Duration::minutes(5).into());
            }
            _ => panic!("expected CalTrigger::Relative"),
        }
    }

    #[test]
    fn alarm() {
        let alarm_str = "BEGIN:VALARM
TRIGGER;VALUE=DATE-TIME:19970317T133000Z
REPEAT:4
DURATION:PT15M
ACTION:DISPLAY
DESCRIPTION:Breakfast meeting with executive\n
  team at 8:30 AM EST.
END:VALARM";
        let mut lines = LineReader::new(alarm_str.as_bytes());
        lines.next().unwrap(); // skip BEGIN:VALARM
        let alarm: CalAlarm =
            CalAlarm::from_lines(&mut lines, Property::new("", vec![], "")).unwrap();
        assert_eq!(
            alarm.trigger,
            CalTrigger::Absolute(CalDate::DateTime(CalDateTime::Utc(
                Utc.with_ymd_and_hms(1997, 3, 17, 13, 30, 0).unwrap()
            )))
        );
        assert_eq!(alarm.repeat, Some(4));
        assert_eq!(alarm.duration, Some(Duration::minutes(15).into()));
        assert_eq!(alarm.action, CalAction::Display);
        assert_eq!(
            alarm.description,
            Some("Breakfast meeting with executive team at 8:30 AM EST.".to_string())
        );

        let res = Vec::new();
        let mut buf_writer = BufWriter::new(res);
        let mut writer = LineWriter::new(&mut buf_writer);
        for prop in alarm.to_props() {
            writer.write_line(&prop.to_string()).unwrap();
        }

        let expected_ical = "BEGIN:VALARM\r
ACTION:DISPLAY\r
TRIGGER:19970317T133000Z\r
DESCRIPTION:Breakfast meeting with executive team at 8:30 AM EST.\r
DURATION:PT15M\r
REPEAT:4\r
END:VALARM\r
";
        assert_eq!(
            String::from_utf8(buf_writer.into_inner().unwrap()).unwrap(),
            expected_ical
        );
    }
}
