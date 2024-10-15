use anyhow::anyhow;
use chrono::{DateTime, Datelike, Duration, Months, NaiveDateTime, TimeZone, Utc, Weekday};
use chrono_tz::Tz;
use icalendar::DatePerhapsTime;
use std::fmt::Debug;
use std::str::FromStr;

use super::ical_date_to_tz;

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Frequency {
    Secondly,
    Minutely,
    Hourly,
    Daily,
    #[default]
    Weekly,
    Monthly,
    Yearly,
}

impl Frequency {
    pub fn advance(&self, now: NaiveDateTime, interval: u32) -> NaiveDateTime {
        match self {
            Self::Secondly => now + Duration::seconds(interval.into()),
            Self::Minutely => now + Duration::minutes(interval.into()),
            Self::Hourly => now + Duration::hours(interval.into()),
            Self::Daily => now + Duration::days(interval.into()),
            Self::Weekly => now + Duration::weeks(interval.into()),
            Self::Monthly => now.checked_add_months(Months::new(interval)).unwrap(),
            Self::Yearly => now.checked_add_months(Months::new(interval * 12)).unwrap(),
        }
    }
}

impl FromStr for Frequency {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "SECONDLY" => Ok(Self::Secondly),
            "MINUTELY" => Ok(Self::Minutely),
            "HOURLY" => Ok(Self::Hourly),
            "DAILY" => Ok(Self::Daily),
            "WEEKLY" => Ok(Self::Weekly),
            "MONTHLY" => Ok(Self::Monthly),
            "YEARLY" => Ok(Self::Yearly),
            _ => Err(anyhow!("unexpected frequency {}", s)),
        }
    }
}

fn parse_weekday(s: &str) -> Result<Weekday, anyhow::Error> {
    match s {
        "SU" => Ok(Weekday::Sun),
        "MO" => Ok(Weekday::Mon),
        "TU" => Ok(Weekday::Tue),
        "WE" => Ok(Weekday::Wed),
        "TH" => Ok(Weekday::Thu),
        "FR" => Ok(Weekday::Fri),
        "SA" => Ok(Weekday::Sat),
        _ => Err(anyhow!("unexpected weekday {}", s)),
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Side {
    Front,
    Back,
}

impl FromStr for Side {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.as_bytes()[0] {
            b'+' => Ok(Self::Front),
            b'-' => Ok(Self::Back),
            _ => Err(anyhow!("unexpected side {}", s)),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct WeekdayDesc {
    day: Weekday,
    nth: Option<(u8, Side)>,
}

impl WeekdayDesc {
    #[cfg(test)]
    pub fn new(day: Weekday, nth: Option<(u8, Side)>) -> Self {
        Self { day, nth }
    }

    pub fn matches(&self, date: DateTime<Tz>, freq: Frequency) -> bool {
        match self.nth {
            None => self.day == date.weekday(),
            Some((n, side)) => false,
        }
    }
}

fn parse_desc_prefix(s: &str) -> Result<(&str, Option<(u8, Side)>), anyhow::Error> {
    let (s, side) = if s.starts_with('-') || s.starts_with('+') {
        (&s[1..], s.parse::<Side>()?)
    } else {
        (s, Side::Front)
    };

    let mut rest = s;
    while rest.as_bytes()[0].is_ascii_digit() {
        rest = &rest[1..];
    }

    let (s, nth) = if rest.len() == s.len() {
        (s, None)
    } else {
        (
            rest,
            Some((s[0..s.len() - rest.len()].parse::<u8>()?, side)),
        )
    };

    Ok((s, nth))
}

impl FromStr for WeekdayDesc {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (s, nth) = parse_desc_prefix(s)?;
        let day = parse_weekday(s)?;
        Ok(Self { day, nth })
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct DayDesc {
    num: u16,
    side: Side,
}

#[cfg(test)]
impl DayDesc {
    pub fn new(num: u16, side: Side) -> Self {
        Self { num, side }
    }
}

impl FromStr for DayDesc {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (s, side) = if s.starts_with('-') || s.starts_with('+') {
            (&s[1..], s.parse::<Side>()?)
        } else {
            (s, Side::Front)
        };
        let num = s.parse::<u16>()?;
        Ok(Self { num, side })
    }
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct RecurrenceRule {
    freq: Frequency,
    until: Option<DatePerhapsTime>,
    count: Option<u8>,
    interval: Option<u8>,
    by_second: Option<Vec<u8>>,
    by_minute: Option<Vec<u8>>,
    by_hour: Option<Vec<u8>>,
    by_day: Option<Vec<WeekdayDesc>>,
    by_mon_day: Option<Vec<DayDesc>>,
    by_year_day: Option<Vec<DayDesc>>,
    by_week_no: Option<Vec<DayDesc>>,
    by_month: Option<Vec<u8>>,
    by_set_pos: Option<Vec<DayDesc>>,
    wk_st: Option<Weekday>,
}

impl RecurrenceRule {
    fn is_included(&self, date: DateTime<Tz>) -> bool {
        if let Some(by_month) = &self.by_month {
            if self.freq <= Frequency::Monthly {
                if !by_month.contains(&(date.month() as u8)) {
                    return false;
                }
            }
        }

        if let Some(by_day) = &self.by_day {
            if self.freq <= Frequency::Daily {
                if by_day
                    .iter()
                    .filter(|wd| wd.matches(date, self.freq))
                    .count()
                    == 0
                {
                    return false;
                }
            }
        }
        true
    }

    pub fn dates_within(&self, start: DateTime<Tz>, end: DateTime<Tz>) -> Vec<DateTime<Tz>> {
        let mut dates = Vec::new();
        let mut date = start.clone();
        let end = if let Some(ref until) = self.until {
            ical_date_to_tz(until, &start.timezone()).min(end)
        } else {
            end
        };
        let interval = self.interval.unwrap_or(1) as u32;

        while date <= end {
            if self.is_included(date) {
                dates.push(date);

                if let Some(count) = self.count {
                    if dates.len() >= count as usize {
                        break;
                    }
                }
            }

            // we basically want to ignore DST here, in the sense that all recurrences of an event
            // that started at 9:00 AM should always be at 9:00 AM as well, regardless of whether
            // DST is on or off. For that reason, we build a NaiveDateTime from the date in the
            // selected timezone, advance it accordingly, and turn it back into a DateTime.
            let next = self.freq.advance(date.naive_local(), interval);
            date = next.and_local_timezone(start.timezone()).unwrap();
        }
        dates
    }
}

fn parse_list<T: FromStr>(s: &str) -> Result<Vec<T>, anyhow::Error> {
    let mut list = Vec::new();
    for item in s.split(',') {
        list.push(
            item.parse()
                .map_err(|_| anyhow!("parsing list item failed"))?,
        );
    }
    Ok(list)
}

impl FromStr for RecurrenceRule {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut rrule = RecurrenceRule::default();
        for part in s.split(';') {
            let mut name_value = part.splitn(2, '=');
            let name = name_value
                .next()
                .ok_or_else(|| anyhow!("malformed rrule"))?;
            let value = name_value
                .next()
                .ok_or_else(|| anyhow!("malformed rrule"))?;
            match name {
                "FREQ" => {
                    rrule.freq = value.parse()?;
                }
                "UNTIL" => {
                    rrule.until = Some(if value.len() <= 8 {
                        DatePerhapsTime::Date(value.parse()?)
                    } else {
                        DatePerhapsTime::DateTime(
                            value.parse().map_err(|_| anyhow!("Invalid datetime"))?,
                        )
                    });
                }
                "COUNT" => {
                    rrule.count = Some(value.parse()?);
                }
                "INTERVAL" => {
                    rrule.interval = Some(value.parse()?);
                }
                "BYSECOND" => {
                    rrule.by_second = Some(parse_list(value)?);
                }
                "BYMINUTE" => {
                    rrule.by_minute = Some(parse_list(value)?);
                }
                "BYHOUR" => {
                    rrule.by_hour = Some(parse_list(value)?);
                }
                "BYDAY" => {
                    rrule.by_day = Some(parse_list(value)?);
                }
                "BYMONTHDAY" => {
                    rrule.by_mon_day = Some(parse_list(value)?);
                }
                "BYYEARDAY" => {
                    rrule.by_year_day = Some(parse_list(value)?);
                }
                "BYWEEKNO" => {
                    rrule.by_week_no = Some(parse_list(value)?);
                }
                "BYMONTH" => {
                    rrule.by_month = Some(parse_list(value)?);
                }
                "BYSETPOS" => {
                    rrule.by_set_pos = Some(parse_list(value)?);
                }
                "WKST" => {
                    rrule.wk_st = Some(parse_weekday(value)?);
                }
                _ => return Err(anyhow!("unexpected rule {}", name)),
            }
        }
        Ok(rrule)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc, Weekday};
    use icalendar::CalendarDateTime;

    use super::*;

    #[test]
    fn parse_weekday_desc() {
        assert_eq!(
            "MO".parse::<WeekdayDesc>().unwrap(),
            WeekdayDesc::new(Weekday::Mon, None)
        );
        assert_eq!(
            "-3SA".parse::<WeekdayDesc>().unwrap(),
            WeekdayDesc::new(Weekday::Sat, Some((3, Side::Back)))
        );
        assert_eq!(
            "+1TU".parse::<WeekdayDesc>().unwrap(),
            WeekdayDesc::new(Weekday::Tue, Some((1, Side::Front)))
        );
        assert_eq!(
            "1FR".parse::<WeekdayDesc>().unwrap(),
            WeekdayDesc::new(Weekday::Fri, Some((1, Side::Front)))
        );
    }

    #[test]
    fn parse_day_desc() {
        assert_eq!(
            "4".parse::<DayDesc>().unwrap(),
            DayDesc::new(4, Side::Front)
        );
        assert_eq!(
            "17".parse::<DayDesc>().unwrap(),
            DayDesc::new(17, Side::Front)
        );
        assert_eq!(
            "-20".parse::<DayDesc>().unwrap(),
            DayDesc::new(20, Side::Back)
        );
        assert_eq!(
            "+19".parse::<DayDesc>().unwrap(),
            DayDesc::new(19, Side::Front)
        );
    }

    #[test]
    fn parse_recur_count() {
        let mut rule = RecurrenceRule::default();
        rule.freq = Frequency::Daily;
        rule.count = Some(10);
        assert_eq!(
            "FREQ=DAILY;COUNT=10".parse::<RecurrenceRule>().unwrap(),
            rule
        );
    }

    #[test]
    fn parse_recur_interval() {
        let mut rule = RecurrenceRule::default();
        rule.freq = Frequency::Monthly;
        rule.interval = Some(2);
        assert_eq!(
            "FREQ=MONTHLY;INTERVAL=2".parse::<RecurrenceRule>().unwrap(),
            rule
        );
    }

    #[test]
    fn parse_recur_until() {
        let mut rule = RecurrenceRule::default();
        rule.freq = Frequency::Daily;
        rule.until = Some(DatePerhapsTime::DateTime(CalendarDateTime::Utc(
            Utc.with_ymd_and_hms(1997, 12, 24, 0, 0, 0).unwrap(),
        )));
        assert_eq!(
            "FREQ=DAILY;UNTIL=19971224T000000Z"
                .parse::<RecurrenceRule>()
                .unwrap(),
            rule
        );
    }

    #[test]
    fn parse_recur_by() {
        let mut rule = RecurrenceRule::default();
        rule.freq = Frequency::Yearly;
        rule.by_month = Some(vec![1]);
        rule.by_set_pos = Some(vec![
            DayDesc::new(2, Side::Front),
            DayDesc::new(5, Side::Front),
        ]);
        rule.by_day = Some(vec![
            WeekdayDesc::new(Weekday::Sun, None),
            WeekdayDesc::new(Weekday::Mon, None),
            WeekdayDesc::new(Weekday::Tue, None),
            WeekdayDesc::new(Weekday::Wed, None),
            WeekdayDesc::new(Weekday::Thu, None),
            WeekdayDesc::new(Weekday::Fri, None),
            WeekdayDesc::new(Weekday::Sat, None),
        ]);

        assert_eq!(
            "FREQ=YEARLY;BYMONTH=1;BYDAY=SU,MO,TU,WE,TH,FR,SA;BYSETPOS=2,+5"
                .parse::<RecurrenceRule>()
                .unwrap(),
            rule
        );
    }

    fn ny_datetime(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> DateTime<Tz> {
        chrono_tz::America::New_York
            .with_ymd_and_hms(year, month, day, hour, min, sec)
            .unwrap()
    }

    #[test]
    fn range_with_count() {
        let start = ny_datetime(1997, 9, 2, 9, 0, 0);
        let rrule = "FREQ=DAILY;COUNT=3".parse::<RecurrenceRule>().unwrap();
        let dates = rrule.dates_within(start, start + Duration::days(20));
        let mut iter = dates.iter();
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 9, 2, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 9, 3, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 9, 4, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_with_until() {
        let start = ny_datetime(1997, 10, 25, 9, 0, 0);
        let rrule = "FREQ=DAILY;UNTIL=19971027T000000Z"
            .parse::<RecurrenceRule>()
            .unwrap();
        let dates = rrule.dates_within(start, start + Duration::days(20));
        let mut iter = dates.iter();
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 10, 25, 9, 0, 0)); // EDT
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 10, 26, 9, 0, 0)); // EST
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_every_other_day() {
        let start = ny_datetime(1997, 10, 25, 9, 0, 0);
        let rrule = "FREQ=DAILY;INTERVAL=2".parse::<RecurrenceRule>().unwrap();
        let dates = rrule.dates_within(start, start + Duration::days(10));
        let mut iter = dates.iter();
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 10, 25, 9, 0, 0)); // EDT
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 10, 27, 9, 0, 0)); // EST
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 10, 29, 9, 0, 0)); // EST
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 10, 31, 9, 0, 0)); // EST
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 11, 2, 9, 0, 0)); // EST
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_every_10_days() {
        let start = ny_datetime(1997, 9, 2, 9, 0, 0);
        let rrule = "FREQ=DAILY;INTERVAL=10;COUNT=5"
            .parse::<RecurrenceRule>()
            .unwrap();
        let dates = rrule.dates_within(start, start + Duration::days(100));
        let mut iter = dates.iter();
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 9, 2, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 9, 12, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 9, 22, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 10, 2, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 10, 12, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_weekly() {
        let start = ny_datetime(1997, 9, 2, 9, 0, 0);
        let rrule = "FREQ=WEEKLY;COUNT=10".parse::<RecurrenceRule>().unwrap();
        let dates = rrule.dates_within(start, start + Duration::weeks(4));
        let mut iter = dates.iter();
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 9, 2, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 9, 9, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 9, 16, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 9, 23, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(1997, 9, 30, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_every_monday() {
        let start = ny_datetime(2024, 9, 2, 9, 0, 0);
        let rrule = "FREQ=DAILY;COUNT=5;BYDAY=MO"
            .parse::<RecurrenceRule>()
            .unwrap();
        let dates = rrule.dates_within(start, start + Duration::weeks(4));
        let mut iter = dates.iter();
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 9, 2, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 9, 9, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 9, 16, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 9, 23, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 9, 30, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }
}
