use std::{collections::HashMap, fmt::Display, io::BufRead, str::FromStr};

use chrono::{DateTime, Duration};
use chrono_tz::Tz;

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
        duration: Duration,
    },
    /// Fires at an absolute time.
    Absolute(CalDate),
}

impl Default for CalTrigger {
    fn default() -> Self {
        Self::Relative {
            related: CalRelated::Start,
            duration: Duration::zero(),
        }
    }
}

impl CalTrigger {
    /// Turns this trigger into a [`Property`].
    pub fn to_prop(&self) -> Property {
        let mut params = Vec::new();
        let value = match self {
            Self::Relative { related, duration } => {
                params.push(Parameter::new("RELATED", format!("{}", related)));
                duration_tostr(*duration)
            }
            Self::Absolute(date) => date.to_string(),
        };
        Property::new("TRIGGER", params, value)
    }
}

fn duration_tostr(mut duration: Duration) -> String {
    let mut s = String::new();
    if duration < Duration::zero() {
        s.push('-');
        duration = -duration;
    }
    s.push('P');
    if duration >= Duration::weeks(1) {
        s.push_str(&format!("{}W", duration.num_weeks()));
        duration -= Duration::weeks(duration.num_weeks());
    }
    if duration >= Duration::days(1) {
        s.push_str(&format!("{}D", duration.num_days()));
        duration -= Duration::days(duration.num_days());
    }
    if duration != Duration::zero() {
        s.push('T');
    }
    if duration >= Duration::hours(1) {
        s.push_str(&format!("{}H", duration.num_hours()));
        duration -= Duration::hours(duration.num_hours());
    }
    if duration >= Duration::minutes(1) {
        s.push_str(&format!("{}M", duration.num_minutes()));
        duration -= Duration::minutes(duration.num_minutes());
    }
    if duration >= Duration::seconds(1) {
        s.push_str(&format!("{}S", duration.num_seconds()));
        duration -= Duration::seconds(duration.num_seconds());
    }
    s
}

fn display_duration(mut duration: Duration) -> String {
    let add = |s: &mut String, add: String| {
        if !s.is_empty() {
            s.push_str(", ");
        }
        s.push_str(&add);
    };

    let mut s = String::new();
    if duration >= Duration::weeks(1) {
        add(&mut s, format!("{} weeks", duration.num_weeks()));
        duration -= Duration::weeks(duration.num_weeks());
    }
    if duration >= Duration::days(1) {
        add(&mut s, format!("{} days", duration.num_days()));
        duration -= Duration::days(duration.num_days());
    }
    if duration >= Duration::hours(1) {
        add(&mut s, format!("{} hours", duration.num_hours()));
        duration -= Duration::hours(duration.num_hours());
    }
    if duration >= Duration::minutes(1) {
        add(&mut s, format!("{} minutes", duration.num_minutes()));
        duration -= Duration::minutes(duration.num_minutes());
    }
    if duration >= Duration::seconds(1) {
        add(&mut s, format!("{} seconds", duration.num_seconds()));
        duration -= Duration::seconds(duration.num_seconds());
    }
    s
}

fn parse_num<'a>(org: &'_ str, d: &'a str) -> Result<(&'a str, i64, char), ParseError> {
    let Some(digits) = d.chars().position(|c| !c.is_ascii_digit()) else {
        return Err(ParseError::InvalidDuration(org.to_string()));
    };

    let num = d[0..digits]
        .parse::<u64>()
        .map_err(ParseError::InvalidNumber)?;
    Ok((&d[digits + 1..], num as i64, d.chars().nth(digits).unwrap()))
}

fn parse_duration(d: &str) -> Result<Duration, ParseError> {
    let org = d;

    // negative or positive duration? the default is positive
    let (d, neg) = if d.starts_with('-') || d.starts_with('+') {
        (&d[1..], d.starts_with('-'))
    } else {
        (d, false)
    };
    if !d.starts_with('P') {
        return Err(ParseError::InvalidDuration(org.to_string()));
    }

    let finish = |d: &str, org: &str, mut duration: Duration| {
        if !d.is_empty() {
            return Err(ParseError::InvalidDuration(org.to_string()));
        }
        if neg {
            duration = -duration;
        }
        Ok(duration)
    };

    let mut duration = Duration::zero();

    let d = &d[1..];
    // note that this is_empty check is not required according to the RFC, but apparently some
    // implementations think that a duration of 'P' is legal. so we support it as well.
    let d = if !d.is_empty() && !d.starts_with('T') {
        let (d, num, t) = parse_num(org, d)?;
        match t {
            'D' => duration += Duration::days(num),
            'W' => {
                duration += Duration::weeks(num);
                return finish(d, org, duration);
            }
            _ => return Err(ParseError::InvalidDuration(org.to_string())),
        }
        d
    } else {
        d
    };

    let d = if !d.is_empty() && d.starts_with('T') {
        let d = &d[1..];

        let (d, num, t) = parse_num(org, d)?;
        match t {
            'H' => duration += Duration::hours(num),
            'M' => duration += Duration::minutes(num),
            'S' => {
                duration += Duration::seconds(num);
                return finish(d, org, duration);
            }
            _ => return Err(ParseError::InvalidDuration(org.to_string())),
        }

        if !d.is_empty() {
            let (d, num, t) = parse_num(org, d)?;
            match t {
                'M' => duration += Duration::minutes(num),
                'S' => {
                    duration += Duration::seconds(num);
                    return finish(d, org, duration);
                }
                _ => return Err(ParseError::InvalidDuration(org.to_string())),
            }

            if !d.is_empty() {
                let (d, num, t) = parse_num(org, d)?;
                match t {
                    'S' => duration += Duration::seconds(num),
                    _ => return Err(ParseError::InvalidDuration(org.to_string())),
                }
                d
            } else {
                d
            }
        } else {
            d
        }
    } else {
        d
    };

    finish(d, org, duration)
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
                duration: parse_duration(prop.value())?,
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
    duration: Option<Duration>,
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
    pub fn duration(&self) -> Option<Duration> {
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
    ) -> Option<DateTime<Tz>> {
        match &self.trigger {
            CalTrigger::Relative { related, duration } => match related {
                CalRelated::Start => start.map(|s| s + *duration),
                CalRelated::End => end.map(|e| e + *duration),
            },
            CalTrigger::Absolute(date) => start.map(|s| date.as_start_with_tz(&s.timezone())),
        }
    }

    /// Returns a human-readable representation of this description.
    pub fn human(&self) -> AlarmHuman<'_> {
        AlarmHuman(self)
    }
}

/// Implements [`Display`](fmt::Display) to create a human-readable representation of a
/// [`CalAlarm`].
///
/// For example, it could say "3rd to last Wednesday".
pub struct AlarmHuman<'a>(&'a CalAlarm);

impl std::fmt::Display for AlarmHuman<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0.trigger {
            CalTrigger::Relative { related, duration } => {
                let (prefix, duration) = if *duration < Duration::zero() {
                    ("before", -*duration)
                } else {
                    ("after", *duration)
                };
                write!(
                    f,
                    "{} {} {}",
                    display_duration(duration),
                    prefix,
                    match related {
                        CalRelated::Start => "start",
                        CalRelated::End => "end",
                    }
                )
            }
            CalTrigger::Absolute(dt) => write!(f, "On {}", dt.fmt_start_with_tz(&Tz::UTC)),
        }
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
            props.push(Property::new("DURATION", vec![], duration_tostr(*duration)));
        }
        if let Some(repeat) = &self.repeat {
            props.push(Property::new("REPEAT", vec![], format!("{}", repeat)));
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
                    comp.duration = Some(parse_duration(prop.value())?);
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
    fn alarms_for_component<'c>(&self, comp: &'c CalComponent) -> Option<Vec<CalAlarm>> {
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

    use chrono::{TimeZone, Utc};

    use crate::{objects::CalDateTime, parser::LineWriter};

    use super::*;

    #[test]
    fn duration() {
        let dur = parse_duration("P15DT5H0M20S").unwrap();
        assert_eq!(dur.num_seconds(), 15 * 86400 + 5 * 3600 + 20);

        let dur = parse_duration("P1DT2H15M").unwrap();
        assert_eq!(dur.num_seconds(), 1 * 86400 + 2 * 3600 + 15 * 60);

        let dur = parse_duration("P1DT2H").unwrap();
        assert_eq!(dur.num_seconds(), 1 * 86400 + 2 * 3600);

        let dur = parse_duration("+P2W").unwrap();
        assert_eq!(dur.num_seconds(), 14 * 86400);

        let dur = parse_duration("-PT2H4M10S").unwrap();
        assert_eq!(dur.num_seconds(), -(2 * 3600 + 4 * 60 + 10));

        let dur = parse_duration("P10D").unwrap();
        assert_eq!(dur.num_seconds(), 10 * 86400);

        let dur = parse_duration("-P10DT4H").unwrap();
        assert_eq!(dur.num_seconds(), -(10 * 86400 + 4 * 3600));
    }

    #[test]
    fn duration_errors() {
        let dur = parse_duration("");
        assert!(matches!(dur, Err(ParseError::InvalidDuration(_))));

        let dur = parse_duration("P2");
        assert!(matches!(dur, Err(ParseError::InvalidDuration(_))));

        let dur = parse_duration("P2W1D");
        assert!(matches!(dur, Err(ParseError::InvalidDuration(_))));
    }

    #[test]
    fn trigger() {
        let prop: Property = "TRIGGER:-PT15M".parse().unwrap();
        let trigger: CalTrigger = prop.try_into().unwrap();
        match trigger {
            CalTrigger::Relative { related, duration } => {
                assert_eq!(related, CalRelated::Start);
                assert_eq!(duration, -Duration::minutes(15));
            }
            _ => panic!("expected CalTrigger::Relative"),
        }

        let prop: Property = "TRIGGER;RELATED=END:PT5M".parse().unwrap();
        let trigger: CalTrigger = prop.try_into().unwrap();
        match trigger {
            CalTrigger::Relative { related, duration } => {
                assert_eq!(related, CalRelated::End);
                assert_eq!(duration, Duration::minutes(5));
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
        assert_eq!(alarm.duration, Some(Duration::minutes(15)));
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
