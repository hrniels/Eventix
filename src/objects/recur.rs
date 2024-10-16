use anyhow::anyhow;
use chrono::offset::LocalResult;
use chrono::{
    DateTime, Datelike, Duration, Months, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc,
    Weekday,
};
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

fn year_day(date: DateTime<Tz>) -> u32 {
    date.date_naive()
        .signed_duration_since(NaiveDate::from_ymd_opt(date.year() - 1, 12, 31).unwrap())
        .num_days() as u32
}

fn year_days(year: i32) -> u32 {
    NaiveDate::from_ymd_opt(year + 1, 1, 1)
        .unwrap()
        .signed_duration_since(NaiveDate::from_ymd_opt(year, 1, 1).unwrap())
        .num_days() as u32
}

fn month_days(year: i32, month: u32) -> u32 {
    if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
    }
    .signed_duration_since(NaiveDate::from_ymd_opt(year, month, 1).unwrap())
    .num_days() as u32
}

impl RecurrenceRule {
    fn limited(&self, date: DateTime<Tz>) -> bool {
        if let Some(by_month) = &self.by_month {
            if self.freq <= Frequency::Monthly {
                if !by_month.contains(&(date.month() as u8)) {
                    return true;
                }
            }
        }

        if let Some(by_yday) = &self.by_year_day {
            if self.freq <= Frequency::Hourly {
                if !by_yday.iter().any(|yd| match yd.side {
                    Side::Front => yd.num as u32 == year_day(date),
                    Side::Back => {
                        let days = year_days(date.year());
                        days - (yd.num - 1) as u32 == year_day(date)
                    }
                }) {
                    return true;
                }
            }
        }
        if let Some(by_mday) = &self.by_mon_day {
            if self.freq <= Frequency::Daily {
                if !by_mday.iter().any(|wd| match wd.side {
                    Side::Front => wd.num as u32 == date.day(),
                    Side::Back => {
                        let days = month_days(date.year(), date.month());
                        days - (wd.num - 1) as u32 == date.day()
                    }
                }) {
                    return true;
                }
            }
        }

        if let Some(by_day) = &self.by_day {
            if self.freq <= Frequency::Daily {
                // num+side is ignored here as this is only applicable for FREQ=MONTHLY|YEARLY
                if !by_day.iter().any(|wd| wd.day == date.weekday()) {
                    return true;
                }
            }
        }

        // TODO ignore if event has DTSTART=DATE
        if let Some(by_hour) = &self.by_hour {
            if self.freq <= Frequency::Hourly {
                if !by_hour.iter().any(|&h| h as u32 == date.hour()) {
                    return true;
                }
            }
        }
        if let Some(by_min) = &self.by_minute {
            if self.freq <= Frequency::Minutely {
                if !by_min.iter().any(|&m| m as u32 == date.minute()) {
                    return true;
                }
            }
        }
        if let Some(by_sec) = &self.by_second {
            if self.freq <= Frequency::Secondly {
                if !by_sec.iter().any(|&s| s as u32 == date.second()) {
                    return true;
                }
            }
        }

        false
    }

    fn expand(&self, start: DateTime<Tz>, date: DateTime<Tz>, res: &mut Vec<DateTime<Tz>>) -> bool {
        let months = [date.month() as u8];
        let mut months = months.as_slice();
        let mut mon_days = vec![date.day() as u16];
        let hours = [date.hour() as u8];
        let mut hours = hours.as_slice();
        let mins = [date.minute() as u8];
        let mut mins = mins.as_slice();
        let secs = [date.second() as u8];
        let mut secs = secs.as_slice();

        if self.by_year_day.is_some() && self.freq > Frequency::Monthly {
            unimplemented!("BYYEARDAY expansion is not supported");
        }
        if self.by_week_no.is_some() && self.freq > Frequency::Monthly {
            unimplemented!("BYWEEKNO expansion is not supported");
        }

        if let Some(by_month) = &self.by_month {
            if self.freq > Frequency::Monthly {
                months = by_month.as_slice();
            }
        }
        if let Some(by_mon_day) = &self.by_mon_day {
            if self.freq >= Frequency::Monthly {
                mon_days = by_mon_day
                    .iter()
                    .map(|md| match md.side {
                        Side::Front => md.num,
                        Side::Back => month_days(date.year(), date.month()) as u16 - (md.num - 1),
                    })
                    .collect();
            }
        }
        if let Some(by_hour) = &self.by_hour {
            if self.freq > Frequency::Hourly {
                hours = by_hour.as_slice();
            }
        }
        if let Some(by_min) = &self.by_minute {
            if self.freq > Frequency::Minutely {
                mins = by_min.as_slice();
            }
        }
        if let Some(by_sec) = &self.by_second {
            if self.freq > Frequency::Secondly {
                secs = by_sec.as_slice();
            }
        }

        for mon in months {
            for d in &mon_days {
                for h in hours {
                    for m in mins {
                        for s in secs {
                            if let Some(ndate) = date.with_month(*mon as u32) {
                                if let Some(ndate) = ndate.with_day(*d as u32) {
                                    if let Some(ndate) = ndate.with_hour(*h as u32) {
                                        if let Some(ndate) = ndate.with_minute(*m as u32) {
                                            if let Some(ndate) = ndate.with_second(*s as u32) {
                                                if ndate >= start {
                                                    res.push(ndate);
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            if let Some(count) = self.count {
                                if res.len() >= count as usize {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
        false
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

        assert!(self.by_set_pos.is_none(), "BYSETPOS is not supported");

        while date <= end {
            if !self.limited(date) {
                if self.expand(start, date, &mut dates) {
                    break;
                }
            }

            // we basically want to ignore DST here, in the sense that all recurrences of an event
            // that started at 9:00 AM should always be at 9:00 AM as well, regardless of whether
            // DST is on or off. For that reason, we build a NaiveDateTime from the date in the
            // selected timezone, advance it accordingly, and turn it back into a DateTime.
            for i in 1.. {
                let next = self.freq.advance(date.naive_local(), interval * i);
                // if the date is not representable in our timezone, just skip it
                if let LocalResult::Single(localdate) = next.and_local_timezone(start.timezone()) {
                    date = localdate;
                    break;
                }
            }
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

    #[test]
    fn range_by_hour_min_sec_limit() {
        let start = ny_datetime(2024, 9, 2, 9, 0, 0);
        let rrule = "FREQ=SECONDLY;COUNT=5;BYHOUR=10,12;BYMINUTE=20,30,40;BYSECOND=10"
            .parse::<RecurrenceRule>()
            .unwrap();
        let dates = rrule.dates_within(start, start + Duration::weeks(4));
        let mut iter = dates.iter();
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 9, 2, 10, 20, 10));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 9, 2, 10, 30, 10));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 9, 2, 10, 40, 10));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 9, 2, 12, 20, 10));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 9, 2, 12, 30, 10));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_monthday_limit() {
        let start = ny_datetime(2024, 9, 2, 9, 0, 0);
        let rrule = "FREQ=DAILY;COUNT=7;BYMONTHDAY=3,10,-1"
            .parse::<RecurrenceRule>()
            .unwrap();
        let dates = rrule.dates_within(start, start + Duration::weeks(12));
        let mut iter = dates.iter();
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 9, 3, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 9, 10, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 9, 30, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 10, 3, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 10, 10, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 10, 31, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 11, 3, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_yearday_limit() {
        let start = ny_datetime(2023, 9, 2, 9, 0, 0);
        let rrule = "FREQ=HOURLY;COUNT=4;BYYEARDAY=2,35,-10;BYHOUR=12"
            .parse::<RecurrenceRule>()
            .unwrap();
        let dates = rrule.dates_within(start, start + Duration::days(500));
        let mut iter = dates.iter();
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 12, 22, 12, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 1, 2, 12, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 2, 4, 12, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 12, 22, 12, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_min_and_sec_expand() {
        let start = ny_datetime(2023, 9, 2, 9, 0, 0);
        let rrule = "FREQ=HOURLY;COUNT=8;BYMINUTE=4,5;BYSECOND=10,20,30"
            .parse::<RecurrenceRule>()
            .unwrap();
        let dates = rrule.dates_within(start, start + Duration::days(1));
        let mut iter = dates.iter();
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 9, 2, 9, 4, 10));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 9, 2, 9, 4, 20));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 9, 2, 9, 4, 30));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 9, 2, 9, 5, 10));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 9, 2, 9, 5, 20));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 9, 2, 9, 5, 30));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 9, 2, 10, 4, 10));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 9, 2, 10, 4, 20));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_hour_expand() {
        let start = ny_datetime(2023, 9, 2, 9, 0, 0);
        let rrule = "FREQ=DAILY;COUNT=5;BYHOUR=4,8"
            .parse::<RecurrenceRule>()
            .unwrap();
        let dates = rrule.dates_within(start, start + Duration::days(5));
        let mut iter = dates.iter();
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 9, 3, 4, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 9, 3, 8, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 9, 4, 4, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 9, 4, 8, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 9, 5, 4, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_monthday_expand() {
        let start = ny_datetime(2023, 9, 2, 9, 0, 0);
        let rrule = "FREQ=MONTHLY;COUNT=5;BYMONTHDAY=1,-1"
            .parse::<RecurrenceRule>()
            .unwrap();
        let dates = rrule.dates_within(start, start + Duration::days(100));
        let mut iter = dates.iter();
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 9, 30, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 10, 1, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 10, 31, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 11, 1, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 11, 30, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_month_expand() {
        let start = ny_datetime(2023, 9, 2, 9, 0, 0);
        let rrule = "FREQ=YEARLY;COUNT=5;BYMONTH=10,11"
            .parse::<RecurrenceRule>()
            .unwrap();
        let dates = rrule.dates_within(start, start + Duration::days(1000));
        let mut iter = dates.iter();
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 10, 2, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2023, 11, 2, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 10, 2, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2024, 11, 2, 9, 0, 0));
        assert_eq!(*iter.next().unwrap(), ny_datetime(2025, 10, 2, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }
}
