use anyhow::anyhow;
use icalendar::DatePerhapsTime;
use std::fmt::Debug;
use std::str::FromStr;

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WeekDay {
    Sunday,
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
}

impl FromStr for WeekDay {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "SU" => Ok(Self::Sunday),
            "MO" => Ok(Self::Monday),
            "TU" => Ok(Self::Tuesday),
            "WE" => Ok(Self::Wednesday),
            "TH" => Ok(Self::Thursday),
            "FR" => Ok(Self::Friday),
            "SA" => Ok(Self::Saturday),
            _ => Err(anyhow!("unexpected weekday {}", s)),
        }
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
pub struct WeekDayDesc {
    day: WeekDay,
    side: Option<Side>,
    ord: Option<u8>,
}

#[cfg(test)]
impl WeekDayDesc {
    pub fn new(day: WeekDay, side: Option<Side>, ord: Option<u8>) -> Self {
        Self { day, side, ord }
    }
}

fn parse_desc_prefix(s: &str) -> Result<(&str, Option<Side>, Option<u8>), anyhow::Error> {
    let (s, side) = if s.starts_with('-') || s.starts_with('+') {
        (&s[1..], Some(s.parse::<Side>()?))
    } else {
        (s, None)
    };

    let mut rest = s;
    while rest.as_bytes()[0].is_ascii_digit() {
        rest = &rest[1..];
    }

    let (s, ord) = if rest.len() == s.len() {
        (s, None)
    } else {
        (rest, Some(s[0..s.len() - rest.len()].parse::<u8>()?))
    };

    Ok((s, side, ord))
}

impl FromStr for WeekDayDesc {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (s, side, ord) = parse_desc_prefix(s)?;
        let day = s.parse::<WeekDay>()?;
        Ok(Self { day, side, ord })
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct DayDesc {
    num: u16,
    side: Option<Side>,
}

#[cfg(test)]
impl DayDesc {
    pub fn new(num: u16, side: Option<Side>) -> Self {
        Self { num, side }
    }
}

impl FromStr for DayDesc {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (s, side) = if s.starts_with('-') || s.starts_with('+') {
            (&s[1..], Some(s.parse::<Side>()?))
        } else {
            (s, None)
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
    by_day: Option<Vec<WeekDayDesc>>,
    by_mon_day: Option<Vec<DayDesc>>,
    by_year_day: Option<Vec<DayDesc>>,
    by_week_no: Option<Vec<DayDesc>>,
    by_month: Option<Vec<u8>>,
    by_set_pos: Option<Vec<DayDesc>>,
    wk_st: Option<WeekDay>,
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
                    rrule.wk_st = Some(value.parse()?);
                }
                _ => return Err(anyhow!("unexpected rule {}", name)),
            }
        }
        Ok(rrule)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use icalendar::CalendarDateTime;

    use super::*;

    #[test]
    fn weekday_desc() {
        assert_eq!(
            "MO".parse::<WeekDayDesc>().unwrap(),
            WeekDayDesc::new(WeekDay::Monday, None, None)
        );
        assert_eq!(
            "-3SA".parse::<WeekDayDesc>().unwrap(),
            WeekDayDesc::new(WeekDay::Saturday, Some(Side::Back), Some(3))
        );
        assert_eq!(
            "+1TU".parse::<WeekDayDesc>().unwrap(),
            WeekDayDesc::new(WeekDay::Tuesday, Some(Side::Front), Some(1))
        );
        assert_eq!(
            "1FR".parse::<WeekDayDesc>().unwrap(),
            WeekDayDesc::new(WeekDay::Friday, None, Some(1))
        );
    }

    #[test]
    fn day_desc() {
        assert_eq!("4".parse::<DayDesc>().unwrap(), DayDesc::new(4, None));
        assert_eq!("17".parse::<DayDesc>().unwrap(), DayDesc::new(17, None));
        assert_eq!(
            "-20".parse::<DayDesc>().unwrap(),
            DayDesc::new(20, Some(Side::Back))
        );
        assert_eq!(
            "+19".parse::<DayDesc>().unwrap(),
            DayDesc::new(19, Some(Side::Front))
        );
    }

    #[test]
    fn recur_count() {
        let mut rule = RecurrenceRule::default();
        rule.freq = Frequency::Daily;
        rule.count = Some(10);
        assert_eq!(
            "FREQ=DAILY;COUNT=10".parse::<RecurrenceRule>().unwrap(),
            rule
        );
    }

    #[test]
    fn recur_interval() {
        let mut rule = RecurrenceRule::default();
        rule.freq = Frequency::Monthly;
        rule.interval = Some(2);
        assert_eq!(
            "FREQ=MONTHLY;INTERVAL=2".parse::<RecurrenceRule>().unwrap(),
            rule
        );
    }

    #[test]
    fn recur_until() {
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
    fn recur_by() {
        let mut rule = RecurrenceRule::default();
        rule.freq = Frequency::Yearly;
        rule.by_month = Some(vec![1]);
        rule.by_set_pos = Some(vec![
            DayDesc::new(2, None),
            DayDesc::new(5, Some(Side::Front)),
        ]);
        rule.by_day = Some(vec![
            WeekDayDesc::new(WeekDay::Sunday, None, None),
            WeekDayDesc::new(WeekDay::Monday, None, None),
            WeekDayDesc::new(WeekDay::Tuesday, None, None),
            WeekDayDesc::new(WeekDay::Wednesday, None, None),
            WeekDayDesc::new(WeekDay::Thursday, None, None),
            WeekDayDesc::new(WeekDay::Friday, None, None),
            WeekDayDesc::new(WeekDay::Saturday, None, None),
        ]);

        assert_eq!(
            "FREQ=YEARLY;BYMONTH=1;BYDAY=SU,MO,TU,WE,TH,FR,SA;BYSETPOS=2,+5"
                .parse::<RecurrenceRule>()
                .unwrap(),
            rule
        );
    }
}
