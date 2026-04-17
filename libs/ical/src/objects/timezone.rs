// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;
use std::io::BufRead;
use std::str::FromStr;

use chrono::{Datelike, Duration, FixedOffset, NaiveDateTime, Offset, TimeZone, Utc, Weekday};
use chrono_tz::{OffsetComponents, OffsetName, Tz};

use crate::objects::{CalDate, CalDateTime, CalRRule, CalRRuleFreq, CalRRuleSide, CalWDayDesc};
use crate::parser::{LineReader, ParseError, Property, PropertyConsumer, PropertyProducer};
use crate::util;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalTimeZone {
    tzid: String,
    last_modified: Option<CalDate>,
    tzurl: Option<String>,
    observances: Vec<CalTimeZoneObservance>,
    props: Vec<Property>,
}

impl CalTimeZone {
    pub fn new(tzid: String) -> Self {
        Self {
            tzid,
            last_modified: None,
            tzurl: None,
            observances: vec![],
            props: vec![],
        }
    }

    pub fn tzid(&self) -> &str {
        &self.tzid
    }

    pub fn last_modified(&self) -> Option<&CalDate> {
        self.last_modified.as_ref()
    }

    pub fn set_last_modified(&mut self, last_modified: Option<CalDate>) {
        self.last_modified = last_modified;
    }

    pub fn tzurl(&self) -> Option<&str> {
        self.tzurl.as_deref()
    }

    pub fn set_tzurl(&mut self, tzurl: Option<String>) {
        self.tzurl = tzurl;
    }

    pub fn observances(&self) -> &[CalTimeZoneObservance] {
        &self.observances
    }

    pub fn properties(&self) -> &[Property] {
        &self.props
    }

    pub fn add_observance(&mut self, observance: CalTimeZoneObservance) {
        self.observances.push(observance);
    }

    pub fn add_standard(&mut self, observance: CalTimeZoneObservance) {
        assert_eq!(observance.kind(), CalTimeZoneObservanceKind::Standard);
        self.add_observance(observance);
    }

    pub fn add_daylight(&mut self, observance: CalTimeZoneObservance) {
        assert_eq!(observance.kind(), CalTimeZoneObservanceKind::Daylight);
        self.add_observance(observance);
    }

    pub fn from_chrono_tz(tzid: &str) -> Option<Self> {
        let tz = tzid.parse::<Tz>().ok()?;
        generate_timezone(tz)
    }

    fn validate(&self) -> Result<(), ParseError> {
        if self.tzid.is_empty() {
            return Err(ParseError::MissingRequiredProp(String::from("TZID")));
        }
        if self.observances.is_empty() {
            return Err(ParseError::MissingRequiredProp(String::from(
                "STANDARD/DAYLIGHT",
            )));
        }
        Ok(())
    }
}

impl PropertyProducer for CalTimeZone {
    fn to_props(&self) -> Vec<Property> {
        let mut props = vec![Property::new("BEGIN", vec![], "VTIMEZONE")];
        props.push(Property::new("TZID", vec![], self.tzid.clone()));
        if let Some(last_modified) = &self.last_modified {
            props.push(last_modified.to_prop("LAST-MODIFIED"));
        }
        if let Some(tzurl) = &self.tzurl {
            props.push(Property::new("TZURL", vec![], tzurl.clone()));
        }
        props.extend(self.props.iter().cloned());
        for observance in &self.observances {
            props.extend(observance.to_props());
        }
        props.push(Property::new("END", vec![], "VTIMEZONE"));
        props
    }
}

fn set_once_prop<T>(slot: &mut Option<T>, name: &str, value: T) -> Result<(), ParseError> {
    if slot.is_some() {
        return Err(ParseError::DuplicateProp(name.to_string()));
    }
    *slot = Some(value);
    Ok(())
}

impl PropertyConsumer for CalTimeZone {
    fn from_lines<R: BufRead>(lines: &mut LineReader<R>, _: Property) -> Result<Self, ParseError>
    where
        Self: Sized,
    {
        let mut tz = CalTimeZone::new("".into());
        loop {
            let Some(line) = lines.next() else {
                break Err(ParseError::UnexpectedEOF);
            };

            let prop = line.parse::<Property>()?;
            match prop.name().as_str() {
                "END" if prop.value() == "VTIMEZONE" => {
                    tz.validate()?;
                    break Ok(tz);
                }
                "TZID" => {
                    if !tz.tzid.is_empty() {
                        return Err(ParseError::DuplicateProp("TZID".to_string()));
                    }
                    tz.tzid = prop.take_value();
                }
                "LAST-MODIFIED" => {
                    let value = prop.try_into()?;
                    set_once_prop(&mut tz.last_modified, "LAST-MODIFIED", value)?;
                }
                "TZURL" => set_once_prop(&mut tz.tzurl, "TZURL", prop.take_value())?,
                "BEGIN" if prop.value() == "STANDARD" => {
                    tz.observances
                        .push(CalTimeZoneObservance::from_lines(lines, prop)?);
                }
                "BEGIN" if prop.value() == "DAYLIGHT" => {
                    tz.observances
                        .push(CalTimeZoneObservance::from_lines(lines, prop)?);
                }
                "BEGIN" => return Err(ParseError::UnexpectedBegin(prop.take_value())),
                _ => {
                    tz.props.push(prop);
                }
            }
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CalTimeZoneObservanceKind {
    Standard,
    Daylight,
}

impl CalTimeZoneObservanceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "STANDARD",
            Self::Daylight => "DAYLIGHT",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalTimeZoneObservance {
    kind: CalTimeZoneObservanceKind,
    dtstart: CalDateTime,
    tzoffset_from: CalUtcOffset,
    tzoffset_to: CalUtcOffset,
    tzname: Vec<String>,
    rrule: Option<CalRRule>,
    rdate: Vec<CalDateTime>,
    props: Vec<Property>,
}

impl CalTimeZoneObservance {
    pub fn new(
        kind: CalTimeZoneObservanceKind,
        dtstart: NaiveDateTime,
        tzoffset_from: CalUtcOffset,
        tzoffset_to: CalUtcOffset,
    ) -> Self {
        Self {
            kind,
            dtstart: CalDateTime::Floating(dtstart),
            tzoffset_from,
            tzoffset_to,
            tzname: vec![],
            rrule: None,
            rdate: vec![],
            props: vec![],
        }
    }

    pub fn kind(&self) -> CalTimeZoneObservanceKind {
        self.kind
    }

    pub fn dtstart(&self) -> &CalDateTime {
        &self.dtstart
    }

    pub fn tzoffset_from(&self) -> CalUtcOffset {
        self.tzoffset_from
    }

    pub fn tzoffset_to(&self) -> CalUtcOffset {
        self.tzoffset_to
    }

    pub fn tzname(&self) -> &[String] {
        &self.tzname
    }

    pub fn add_tzname(&mut self, tzname: String) {
        self.tzname.push(tzname);
    }

    pub fn rrule(&self) -> Option<&CalRRule> {
        self.rrule.as_ref()
    }

    pub fn set_rrule(&mut self, rrule: Option<CalRRule>) {
        self.rrule = rrule;
    }

    pub fn rdate(&self) -> &[CalDateTime] {
        &self.rdate
    }

    pub fn add_rdate(&mut self, rdate: NaiveDateTime) {
        self.rdate.push(CalDateTime::Floating(rdate));
    }

    fn validate_dtstart(prop: Property) -> Result<CalDateTime, ParseError> {
        let prop_name = prop.name().clone();
        let date: CalDate = prop.try_into()?;
        match date {
            CalDate::DateTime(CalDateTime::Floating(dt)) => Ok(CalDateTime::Floating(dt)),
            _ => Err(ParseError::InvalidDate(format!(
                "{} must be a local DATE-TIME without TZID or Z",
                prop_name
            ))),
        }
    }

    fn validate_rdate(prop: Property) -> Result<Vec<CalDateTime>, ParseError> {
        let mut dates = Vec::new();
        for date in prop.value().split(',') {
            let date_prop = Property::new(prop.name(), prop.params().to_vec(), date);
            let parsed: CalDate = date_prop.try_into()?;
            match parsed {
                CalDate::DateTime(CalDateTime::Floating(dt)) => {
                    dates.push(CalDateTime::Floating(dt));
                }
                _ => {
                    return Err(ParseError::InvalidDate(String::from(
                        "RDATE in VTIMEZONE must be local DATE-TIME",
                    )));
                }
            }
        }
        Ok(dates)
    }

    fn validate(&self) -> Result<(), ParseError> {
        if self.rrule.is_none() && self.rdate.is_empty() {
            return Ok(());
        }
        Ok(())
    }
}

impl PropertyProducer for CalTimeZoneObservance {
    fn to_props(&self) -> Vec<Property> {
        let mut props = vec![Property::new("BEGIN", vec![], self.kind.as_str())];
        props.push(self.dtstart.to_prop("DTSTART"));
        props.push(Property::new(
            "TZOFFSETFROM",
            vec![],
            self.tzoffset_from.to_string(),
        ));
        props.push(Property::new(
            "TZOFFSETTO",
            vec![],
            self.tzoffset_to.to_string(),
        ));
        for tzname in &self.tzname {
            props.push(Property::new("TZNAME", vec![], tzname.clone()));
        }
        if let Some(rrule) = &self.rrule {
            props.push(Property::new_escaped("RRULE", vec![], rrule.to_string()));
        }
        for rdate in &self.rdate {
            props.push(rdate.to_prop("RDATE"));
        }
        props.extend(self.props.iter().cloned());
        props.push(Property::new("END", vec![], self.kind.as_str()));
        props
    }
}

impl PropertyConsumer for CalTimeZoneObservance {
    fn from_lines<R: BufRead>(lines: &mut LineReader<R>, prop: Property) -> Result<Self, ParseError>
    where
        Self: Sized,
    {
        let kind = match prop.value().as_str() {
            "STANDARD" => CalTimeZoneObservanceKind::Standard,
            "DAYLIGHT" => CalTimeZoneObservanceKind::Daylight,
            _ => return Err(ParseError::UnexpectedBegin(prop.take_value())),
        };

        let mut dtstart = None;
        let mut tzoffset_from = None;
        let mut tzoffset_to = None;
        let mut tzname = Vec::new();
        let mut rrule = None;
        let mut rdate = Vec::new();
        let mut props = Vec::new();

        loop {
            let Some(line) = lines.next() else {
                break Err(ParseError::UnexpectedEOF);
            };

            let prop = line.parse::<Property>()?;
            match prop.name().as_str() {
                "END" if prop.value() == kind.as_str() => {
                    let dtstart = dtstart
                        .ok_or_else(|| ParseError::MissingRequiredProp(String::from("DTSTART")))?;
                    let tzoffset_from = tzoffset_from.ok_or_else(|| {
                        ParseError::MissingRequiredProp(String::from("TZOFFSETFROM"))
                    })?;
                    let tzoffset_to = tzoffset_to.ok_or_else(|| {
                        ParseError::MissingRequiredProp(String::from("TZOFFSETTO"))
                    })?;
                    let observance = Self {
                        kind,
                        dtstart,
                        tzoffset_from,
                        tzoffset_to,
                        tzname,
                        rrule,
                        rdate,
                        props,
                    };
                    observance.validate()?;
                    break Ok(observance);
                }
                "DTSTART" => {
                    let value = Self::validate_dtstart(prop)?;
                    set_once_prop(&mut dtstart, "DTSTART", value)?;
                }
                "TZOFFSETFROM" => {
                    let value = prop.value().parse()?;
                    set_once_prop(&mut tzoffset_from, "TZOFFSETFROM", value)?;
                }
                "TZOFFSETTO" => {
                    let value = prop.value().parse()?;
                    set_once_prop(&mut tzoffset_to, "TZOFFSETTO", value)?;
                }
                "TZNAME" => tzname.push(prop.take_value()),
                "RRULE" => {
                    let value = prop.value().parse()?;
                    set_once_prop(&mut rrule, "RRULE", value)?;
                }
                "RDATE" => rdate.extend(Self::validate_rdate(prop)?),
                "BEGIN" => return Err(ParseError::UnexpectedBegin(prop.take_value())),
                _ => props.push(prop),
            }
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CalUtcOffset {
    seconds: i32,
}

impl CalUtcOffset {
    pub fn from_seconds(seconds: i32) -> Result<Self, ParseError> {
        let abs = seconds.abs();
        let secs = abs % 60;
        let mins = (abs / 60) % 60;
        let hours = abs / 3600;
        if mins >= 60 || secs >= 60 {
            return Err(ParseError::InvalidUtcOffset(seconds.to_string()));
        }
        if seconds < 0 && abs == 0 {
            return Err(ParseError::InvalidUtcOffset(String::from("-0000")));
        }
        if hours > 23 {
            return Err(ParseError::InvalidUtcOffset(seconds.to_string()));
        }
        Ok(Self { seconds })
    }

    pub fn as_seconds(self) -> i32 {
        self.seconds
    }

    pub fn as_fixed_offset(self) -> FixedOffset {
        FixedOffset::east_opt(self.seconds).expect("valid UTC offset")
    }
}

impl fmt::Display for CalUtcOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.seconds < 0 { '-' } else { '+' };
        let abs = self.seconds.abs();
        let hours = abs / 3600;
        let minutes = (abs % 3600) / 60;
        let seconds = abs % 60;
        if seconds == 0 {
            write!(f, "{sign}{hours:02}{minutes:02}")
        } else {
            write!(f, "{sign}{hours:02}{minutes:02}{seconds:02}")
        }
    }
}

impl FromStr for CalUtcOffset {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 5 && s.len() != 7 {
            return Err(ParseError::InvalidUtcOffset(s.to_string()));
        }
        let sign = match &s[0..1] {
            "+" => 1,
            "-" => -1,
            _ => return Err(ParseError::InvalidUtcOffset(s.to_string())),
        };
        if s == "-0000" || s == "-000000" {
            return Err(ParseError::InvalidUtcOffset(s.to_string()));
        }

        let hours = s[1..3]
            .parse::<i32>()
            .map_err(|_| ParseError::InvalidUtcOffset(s.to_string()))?;
        let minutes = s[3..5]
            .parse::<i32>()
            .map_err(|_| ParseError::InvalidUtcOffset(s.to_string()))?;
        let seconds = if s.len() == 7 {
            s[5..7]
                .parse::<i32>()
                .map_err(|_| ParseError::InvalidUtcOffset(s.to_string()))?
        } else {
            0
        };

        if minutes >= 60 || seconds >= 60 {
            return Err(ParseError::InvalidUtcOffset(s.to_string()));
        }

        Self::from_seconds(sign * (hours * 3600 + minutes * 60 + seconds))
            .map_err(|_| ParseError::InvalidUtcOffset(s.to_string()))
    }
}

fn generate_timezone(tz: Tz) -> Option<CalTimeZone> {
    let transitions = detect_recent_transitions(tz)?;

    let mut timezone = CalTimeZone::new(tz.name().to_string());
    timezone.set_last_modified(Some(CalDate::DateTime(CalDateTime::Utc(Utc::now()))));

    if transitions.is_empty() {
        let sample = tz
            .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
            .single()
            .or_else(|| tz.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single())?;
        timezone.add_standard(build_single_observance(sample));
        return Some(timezone);
    }

    let standard = build_rrule_observance(&transitions, false)?;
    timezone.add_standard(standard);

    if let Some(daylight) = build_rrule_observance(&transitions, true) {
        timezone.add_daylight(daylight);
    }

    Some(timezone)
}

fn build_single_observance(sample: chrono::DateTime<Tz>) -> CalTimeZoneObservance {
    let offset = sample.offset();
    let total = CalUtcOffset::from_seconds(offset.fix().local_minus_utc()).unwrap();
    let mut obs = CalTimeZoneObservance::new(
        CalTimeZoneObservanceKind::Standard,
        sample.naive_local(),
        total,
        total,
    );
    if let Some(name) = offset.abbreviation() {
        obs.add_tzname(name.to_string());
    }
    obs
}

#[derive(Clone)]
struct TransitionInfo {
    year: i32,
    month: u8,
    weekday: Weekday,
    nth: u8,
    kind: CalTimeZoneObservanceKind,
    start: NaiveDateTime,
    offset_from: CalUtcOffset,
    offset_to: CalUtcOffset,
    tzname: Option<String>,
}

fn build_rrule_observance(
    transitions: &[TransitionInfo],
    dst: bool,
) -> Option<CalTimeZoneObservance> {
    let filtered: Vec<_> = transitions
        .iter()
        .filter(|t| (t.kind == CalTimeZoneObservanceKind::Daylight) == dst)
        .cloned()
        .collect();
    if filtered.is_empty() {
        return None;
    }

    // if any transition is different than the first, we give up here
    let first = filtered.first()?;
    if filtered.iter().any(|t| {
        t.month != first.month
            || t.weekday != first.weekday
            || t.nth != first.nth
            || t.start.time() != first.start.time()
            || t.offset_from != first.offset_from
            || t.offset_to != first.offset_to
    }) {
        return None;
    }

    let mut obs =
        CalTimeZoneObservance::new(first.kind, first.start, first.offset_from, first.offset_to);
    if let Some(name) = &first.tzname {
        obs.add_tzname(name.clone());
    }

    let mut rule = CalRRule::default();
    rule.set_frequency(CalRRuleFreq::Yearly);
    rule.set_by_month(Some(vec![first.month]));
    rule.set_by_day(Some(vec![CalWDayDesc::new(
        first.weekday,
        Some((first.nth, CalRRuleSide::End)),
    )]));
    obs.set_rrule(Some(rule));
    Some(obs)
}

fn detect_recent_transitions(tz: Tz) -> Option<Vec<TransitionInfo>> {
    let start_year = Utc::now().year() - 2;
    let end_year = Utc::now().year() + 3;
    let start = Utc.with_ymd_and_hms(start_year, 1, 1, 0, 0, 0).single()?;
    let end = Utc.with_ymd_and_hms(end_year, 1, 1, 0, 0, 0).single()?;

    let mut current = start;
    let mut current_offset = current.with_timezone(&tz).offset().fix().local_minus_utc();
    let mut transitions = Vec::new();

    while current < end {
        let next = (current + Duration::days(7)).min(end);
        let next_offset = next.with_timezone(&tz).offset().fix().local_minus_utc();
        if next_offset != current_offset {
            let transition = find_transition(tz, current, next)?;
            let before = (transition - Duration::minutes(1)).with_timezone(&tz);
            let after = transition.with_timezone(&tz);
            let local = after.naive_local();
            let day = local.date();
            let nth_from_end =
                ((util::month_days(day.year(), day.month()) - day.day()) / 7 + 1) as u8;
            transitions.push(TransitionInfo {
                year: day.year(),
                month: day.month() as u8,
                weekday: day.weekday(),
                nth: nth_from_end,
                kind: if after.offset().dst_offset() > Duration::zero() {
                    CalTimeZoneObservanceKind::Daylight
                } else {
                    CalTimeZoneObservanceKind::Standard
                },
                start: local,
                offset_from: CalUtcOffset::from_seconds(before.offset().fix().local_minus_utc())
                    .ok()?,
                offset_to: CalUtcOffset::from_seconds(after.offset().fix().local_minus_utc())
                    .ok()?,
                tzname: after.offset().abbreviation().map(ToString::to_string),
            });
            current_offset = next_offset;
        }
        current = next;
    }

    Some(
        transitions
            .into_iter()
            .filter(|t| t.year > start_year)
            .collect(),
    )
}

fn find_transition(
    tz: Tz,
    mut start: chrono::DateTime<Utc>,
    mut end: chrono::DateTime<Utc>,
) -> Option<chrono::DateTime<Utc>> {
    let start_offset = start.with_timezone(&tz).offset().fix().local_minus_utc();
    while (end - start) > Duration::minutes(1) {
        let mid = start + Duration::seconds((end - start).num_seconds() / 2);
        let mid_offset = mid.with_timezone(&tz).offset().fix().local_minus_utc();
        if mid_offset == start_offset {
            start = mid;
        } else {
            end = mid;
        }
    }
    Some(end)
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use crate::objects::{
        CalDate, CalDateTime, CalTimeZone, CalTimeZoneObservance, CalTimeZoneObservanceKind,
        CalUtcOffset, Calendar,
    };
    use crate::parser::ParseError;

    fn minimal_observance(kind: &str, dtstart: &str, from: &str, to: &str) -> String {
        format!(
            "BEGIN:{kind}\nDTSTART:{dtstart}\nTZOFFSETFROM:{from}\nTZOFFSETTO:{to}\nEND:{kind}\n"
        )
    }

    #[test]
    fn timezones_returns_timezone_components() {
        let input = format!(
            "BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VTIMEZONE\nTZID:America/New_York\n{}END:VTIMEZONE\nBEGIN:VTIMEZONE\nTZID:Europe/London\n{}END:VTIMEZONE\nEND:VCALENDAR\n",
            minimal_observance("STANDARD", "19701101T020000", "-0400", "-0500"),
            minimal_observance("STANDARD", "19701025T020000", "+0100", "+0000"),
        );

        let cal = input.parse::<Calendar>().unwrap();
        let tzs = cal.timezones();
        assert_eq!(tzs.len(), 2);
        assert_eq!(tzs[0].tzid(), "America/New_York");
        assert_eq!(tzs[1].tzid(), "Europe/London");
    }

    #[test]
    fn timezone_serialization_includes_props() {
        let input = format!(
            "BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VTIMEZONE\nTZID:America/Chicago\n{}X-CUSTOM-PROP:custom-value\nEND:VTIMEZONE\nEND:VCALENDAR\n",
            minimal_observance("STANDARD", "19701101T020000", "-0500", "-0600"),
        );

        let cal = input.parse::<Calendar>().unwrap();
        let tz = &cal.timezones()[0];
        assert_eq!(tz.tzid(), "America/Chicago");
        assert_eq!(tz.properties().len(), 1);
        assert_eq!(tz.properties()[0].name(), "X-CUSTOM-PROP");

        let mut buf = Vec::new();
        cal.write(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("TZID:America/Chicago\r\n"));
        assert!(output.contains("X-CUSTOM-PROP:custom-value\r\n"));
        assert!(output.contains(
            "BEGIN:STANDARD\r\nDTSTART:19701101T020000\r\nTZOFFSETFROM:-0500\r\nTZOFFSETTO:-0600\r\nEND:STANDARD\r\n"
        ));
    }

    #[test]
    fn timezone_parse_and_roundtrip_with_observances() {
        let input = "BEGIN:VCALENDAR\n\
VERSION:2.0\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/Berlin\n\
LAST-MODIFIED:20240101T120000Z\n\
TZURL:https://example.com/tz/Europe-Berlin.ics\n\
BEGIN:STANDARD\n\
DTSTART:19701025T030000\n\
TZOFFSETFROM:+0200\n\
TZOFFSETTO:+0100\n\
TZNAME:CET\n\
RRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\n\
END:STANDARD\n\
BEGIN:DAYLIGHT\n\
DTSTART:19700329T020000\n\
TZOFFSETFROM:+0100\n\
TZOFFSETTO:+0200\n\
TZNAME:CEST\n\
RDATE:19800330T020000,19810329T020000\n\
END:DAYLIGHT\n\
END:VTIMEZONE\n\
END:VCALENDAR\n";

        let cal = input.parse::<Calendar>().unwrap();
        let tz = &cal.timezones()[0];
        assert_eq!(tz.tzid(), "Europe/Berlin");
        assert_eq!(tz.observances().len(), 2);
        assert_eq!(tz.tzurl(), Some("https://example.com/tz/Europe-Berlin.ics"));

        let standard = &tz.observances()[0];
        assert_eq!(standard.kind(), CalTimeZoneObservanceKind::Standard);
        assert_eq!(standard.tzoffset_from(), "+0200".parse().unwrap());
        assert_eq!(standard.tzoffset_to(), "+0100".parse().unwrap());
        assert_eq!(standard.tzname(), ["CET".to_string()].as_slice());
        assert!(standard.rrule().is_some());

        let daylight = &tz.observances()[1];
        assert_eq!(daylight.kind(), CalTimeZoneObservanceKind::Daylight);
        assert_eq!(daylight.rdate().len(), 2);

        let mut buf = Vec::new();
        cal.write(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("BEGIN:STANDARD\r\nDTSTART:19701025T030000\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\n"));
        assert!(output.contains("TZNAME:CET\r\n"));
        assert!(output.contains("RRULE:FREQ=YEARLY"));
        assert!(output.contains("BYMONTH=10"));
        assert!(output.contains("BYDAY=-1SU"));
        assert!(output.contains("BEGIN:DAYLIGHT\r\nDTSTART:19700329T020000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\nTZNAME:CEST\r\nRDATE:19800330T020000\r\nRDATE:19810329T020000\r\nEND:DAYLIGHT\r\n"));
    }

    #[test]
    fn timezone_creation_serializes_full_structure() {
        let mut tz = CalTimeZone::new("Europe/Berlin".to_string());
        tz.set_last_modified(Some(CalDate::DateTime(CalDateTime::Utc(
            NaiveDate::from_ymd_opt(2024, 1, 1)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap()
                .and_utc(),
        ))));
        tz.set_tzurl(Some("https://example.com/tz/Europe-Berlin.ics".to_string()));

        let mut standard = CalTimeZoneObservance::new(
            CalTimeZoneObservanceKind::Standard,
            NaiveDate::from_ymd_opt(1970, 10, 25)
                .unwrap()
                .and_hms_opt(3, 0, 0)
                .unwrap(),
            "+0200".parse().unwrap(),
            "+0100".parse().unwrap(),
        );
        standard.add_tzname("CET".to_string());
        standard.set_rrule(Some("FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU".parse().unwrap()));
        tz.add_standard(standard);

        let mut daylight = CalTimeZoneObservance::new(
            CalTimeZoneObservanceKind::Daylight,
            NaiveDate::from_ymd_opt(1970, 3, 29)
                .unwrap()
                .and_hms_opt(2, 0, 0)
                .unwrap(),
            "+0100".parse().unwrap(),
            "+0200".parse().unwrap(),
        );
        daylight.add_tzname("CEST".to_string());
        daylight.add_rdate(
            NaiveDate::from_ymd_opt(1980, 3, 30)
                .unwrap()
                .and_hms_opt(2, 0, 0)
                .unwrap(),
        );
        tz.add_daylight(daylight);

        let mut cal = Calendar::default();
        cal.add_timezone(tz);

        let mut buf = Vec::new();
        cal.write(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("TZID:Europe/Berlin\r\nLAST-MODIFIED:20240101T120000Z\r\nTZURL:https://example.com/tz/Europe-Berlin.ics\r\n"));
        assert!(output.contains("BEGIN:STANDARD\r\nDTSTART:19701025T030000\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\n"));
        assert!(output.contains("TZNAME:CET\r\n"));
        assert!(output.contains("RRULE:FREQ=YEARLY"));
        assert!(output.contains("BYMONTH=10"));
        assert!(output.contains("BYDAY=-1SU"));
        assert!(output.contains("BEGIN:DAYLIGHT\r\nDTSTART:19700329T020000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\n"));
        assert!(output.contains("TZNAME:CEST\r\n"));
        assert!(output.contains("RDATE:19800330T020000\r\n"));
    }

    #[test]
    fn timezone_requires_observance() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/Berlin\n\
END:VTIMEZONE\n\
END:VCALENDAR\n";

        let err = input.parse::<Calendar>().unwrap_err();
        assert_eq!(
            err,
            ParseError::MissingRequiredProp("STANDARD/DAYLIGHT".to_string())
        );
    }

    #[test]
    fn timezone_requires_tzid() {
        let input = format!(
            "BEGIN:VCALENDAR\nBEGIN:VTIMEZONE\n{}END:VTIMEZONE\nEND:VCALENDAR\n",
            minimal_observance("STANDARD", "19701025T030000", "+0200", "+0100")
        );

        let err = input.parse::<Calendar>().unwrap_err();
        assert_eq!(err, ParseError::MissingRequiredProp("TZID".to_string()));
    }

    #[test]
    fn observance_requires_required_properties() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/Berlin\n\
BEGIN:STANDARD\n\
DTSTART:19701025T030000\n\
TZOFFSETTO:+0100\n\
END:STANDARD\n\
END:VTIMEZONE\n\
END:VCALENDAR\n";

        let err = input.parse::<Calendar>().unwrap_err();
        assert_eq!(
            err,
            ParseError::MissingRequiredProp("TZOFFSETFROM".to_string())
        );
    }

    #[test]
    fn observance_dtstart_must_be_local_datetime() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/Berlin\n\
BEGIN:STANDARD\n\
DTSTART;TZID=Europe/Berlin:19701025T030000\n\
TZOFFSETFROM:+0200\n\
TZOFFSETTO:+0100\n\
END:STANDARD\n\
END:VTIMEZONE\n\
END:VCALENDAR\n";

        let err = input.parse::<Calendar>().unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidDate(
                "DTSTART must be a local DATE-TIME without TZID or Z".to_string()
            )
        );
    }

    #[test]
    fn observance_rdate_must_be_local_datetime() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/Berlin\n\
BEGIN:STANDARD\n\
DTSTART:19701025T030000\n\
TZOFFSETFROM:+0200\n\
TZOFFSETTO:+0100\n\
RDATE:19701025\n\
END:STANDARD\n\
END:VTIMEZONE\n\
END:VCALENDAR\n";

        let err = input.parse::<Calendar>().unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidDate("RDATE in VTIMEZONE must be local DATE-TIME".to_string())
        );
    }

    #[test]
    fn timezone_rejects_duplicate_singleton_properties() {
        let input = format!(
            "BEGIN:VCALENDAR\nBEGIN:VTIMEZONE\nTZID:Europe/Berlin\nTZID:Europe/Paris\n{}END:VTIMEZONE\nEND:VCALENDAR\n",
            minimal_observance("STANDARD", "19701025T030000", "+0200", "+0100")
        );

        let err = input.parse::<Calendar>().unwrap_err();
        assert_eq!(err, ParseError::DuplicateProp("TZID".to_string()));
    }

    #[test]
    fn utc_offset_parses_and_formats() {
        let pos: CalUtcOffset = "+0530".parse().unwrap();
        assert_eq!(pos.as_seconds(), 19_800);
        assert_eq!(pos.to_string(), "+0530");

        let neg: CalUtcOffset = "-023045".parse().unwrap();
        assert_eq!(neg.as_seconds(), -(2 * 3600 + 30 * 60 + 45));
        assert_eq!(neg.to_string(), "-023045");
        assert_eq!(
            neg.as_fixed_offset().local_minus_utc(),
            -(2 * 3600 + 30 * 60 + 45)
        );
    }

    #[test]
    fn utc_offset_rejects_invalid_values() {
        assert_eq!(
            "-0000".parse::<CalUtcOffset>().unwrap_err(),
            ParseError::InvalidUtcOffset("-0000".to_string())
        );
        assert_eq!(
            "+126060".parse::<CalUtcOffset>().unwrap_err(),
            ParseError::InvalidUtcOffset("+126060".to_string())
        );
        assert_eq!(
            "+2400".parse::<CalUtcOffset>().unwrap_err(),
            ParseError::InvalidUtcOffset("+2400".to_string())
        );
    }

    #[test]
    fn from_chrono_tz_generates_single_standard_for_fixed_offset_zone() {
        let tz = CalTimeZone::from_chrono_tz("Asia/Kathmandu").unwrap();

        assert_eq!(tz.tzid(), "Asia/Kathmandu");
        assert_eq!(tz.observances().len(), 1);

        let standard = &tz.observances()[0];
        assert_eq!(standard.kind(), CalTimeZoneObservanceKind::Standard);
        assert_eq!(standard.tzoffset_from(), "+0545".parse().unwrap());
        assert_eq!(standard.tzoffset_to(), "+0545".parse().unwrap());
        assert!(standard.rrule().is_none());
        assert!(
            standard.tzname().is_empty() || standard.tzname() == ["+0545".to_string()].as_slice()
        );
    }

    #[test]
    fn populate_timezones_skips_irregular_half_hour_dst_zone() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:test\n\
DTSTAMP:20250101T000000Z\n\
DTSTART;TZID=Australia/Lord_Howe:20250401T100000\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        cal.populate_timezones();

        assert!(cal.timezones().is_empty());
    }

    #[test]
    fn populate_timezones_leaves_unparsable_custom_tzid_uncovered() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:test\n\
DTSTAMP:20250101T000000Z\n\
DTSTART;TZID=My/Custom-TZ:20250330T100000\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        cal.populate_timezones();

        assert!(cal.timezones().is_empty());
    }

    #[test]
    fn populate_timezones_handles_fixed_offset_zone_without_rrule() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:test\n\
DTSTAMP:20250101T000000Z\n\
DTSTART;TZID=Asia/Kathmandu:20250330T100000\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        cal.populate_timezones();

        assert_eq!(cal.timezones().len(), 1);
        let timezone = &cal.timezones()[0];
        assert_eq!(timezone.observances().len(), 1);
        assert!(timezone.observances()[0].rrule().is_none());
    }
}
