// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{fmt::Display, ops::Deref, str::FromStr};

use formatx::formatx;

use chrono::Duration;

use crate::{objects::CalLocale, parser::ParseError};

/// A duration for calendar objects.
///
/// Implements [`Display`] and [`FromStr`] to be turned into a string representation and vice
/// versa. A human representation can be retrieved via [`CalDuration::human`]. Note that it can be
/// constructed from a [`chrono::Duration`] and also implements [`Deref`] into
/// [`chrono::Duration`].
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.2.5>.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalDuration {
    delta: Duration,
}

impl CalDuration {
    /// Returns a human-readable representation of the duration
    pub fn human<'l>(&self, locale: &'l dyn CalLocale) -> HumanDuration<'l> {
        HumanDuration {
            delta: self.delta,
            locale,
        }
    }
}

impl From<Duration> for CalDuration {
    fn from(value: Duration) -> Self {
        Self { delta: value }
    }
}

impl Deref for CalDuration {
    type Target = Duration;

    fn deref(&self) -> &Self::Target {
        &self.delta
    }
}

impl Display for CalDuration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut delta = self.delta;
        if delta < Duration::zero() {
            write!(f, "-")?;
            delta = -delta;
        }
        write!(f, "P")?;
        if delta >= Duration::days(1) {
            write!(f, "{}D", delta.num_days())?;
            delta -= Duration::days(delta.num_days());
        }
        if delta != Duration::zero() {
            write!(f, "T")?;
        }
        if delta >= Duration::hours(1) {
            write!(f, "{}H", delta.num_hours())?;
            delta -= Duration::hours(delta.num_hours());
        }
        if delta >= Duration::minutes(1) {
            write!(f, "{}M", delta.num_minutes())?;
            delta -= Duration::minutes(delta.num_minutes());
        }
        if delta >= Duration::seconds(1) {
            write!(f, "{}S", delta.num_seconds())?;
            delta -= Duration::seconds(delta.num_seconds());
        }
        Ok(())
    }
}

/// A human readable representation of a duration.
#[derive(Debug)]
pub struct HumanDuration<'l> {
    delta: Duration,
    locale: &'l dyn CalLocale,
}

impl Display for HumanDuration<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut duration = self.delta;
        let add = |s: &mut String, add: String| {
            if !s.is_empty() {
                s.push_str(", ");
            }
            s.push_str(&add);
        };

        let mut s = String::new();
        if duration >= Duration::weeks(1) {
            add(
                &mut s,
                formatx!(self.locale.translate("{} weeks"), duration.num_weeks()).unwrap(),
            );
            duration -= Duration::weeks(duration.num_weeks());
        }
        if duration >= Duration::days(1) {
            add(
                &mut s,
                formatx!(self.locale.translate("{} days"), duration.num_days()).unwrap(),
            );
            duration -= Duration::days(duration.num_days());
        }
        if duration >= Duration::hours(1) {
            add(
                &mut s,
                formatx!(self.locale.translate("{} hours"), duration.num_hours()).unwrap(),
            );
            duration -= Duration::hours(duration.num_hours());
        }
        if duration >= Duration::minutes(1) {
            add(
                &mut s,
                formatx!(self.locale.translate("{} minutes"), duration.num_minutes()).unwrap(),
            );
            duration -= Duration::minutes(duration.num_minutes());
        }
        if duration >= Duration::seconds(1) {
            add(
                &mut s,
                formatx!(self.locale.translate("{} seconds"), duration.num_seconds()).unwrap(),
            );
            duration -= Duration::seconds(duration.num_seconds());
        }
        write!(f, "{s}")
    }
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

impl FromStr for CalDuration {
    type Err = ParseError;

    fn from_str(d: &str) -> Result<Self, Self::Err> {
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
            Ok(CalDuration { delta: duration })
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
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::CalDuration;
    use crate::objects::CalLocaleEn;
    use crate::parser::ParseError;

    #[test]
    fn display_formats_complete_positive_negative_and_zero_durations() {
        let full = CalDuration::from(
            Duration::days(2) + Duration::hours(3) + Duration::minutes(4) + Duration::seconds(5),
        );
        assert_eq!(full.to_string(), "P2DT3H4M5S");

        let negative = CalDuration::from(
            -(Duration::days(2) + Duration::hours(3) + Duration::minutes(4) + Duration::seconds(5)),
        );
        assert_eq!(negative.to_string(), "-P2DT3H4M5S");

        let zero = CalDuration::from(Duration::zero());
        assert_eq!(zero.to_string(), "P");
    }

    #[test]
    fn human_display_lists_all_units_in_order() {
        let duration = CalDuration::from(
            Duration::weeks(1)
                + Duration::days(2)
                + Duration::hours(3)
                + Duration::minutes(4)
                + Duration::seconds(5),
        );

        let locale = CalLocaleEn;
        assert_eq!(
            duration.human(&locale).to_string(),
            "1 weeks, 2 days, 3 hours, 4 minutes, 5 seconds"
        );
    }

    #[test]
    fn parse_supports_seconds_and_mixed_time_units() {
        let seconds_only = "PT45S".parse::<CalDuration>().unwrap();
        assert_eq!(seconds_only.to_string(), "PT45S");
        assert_eq!(*seconds_only, Duration::seconds(45));

        let mixed = "PT1H30S".parse::<CalDuration>().unwrap();
        assert_eq!(mixed.to_string(), "PT1H30S");
        assert_eq!(*mixed, Duration::hours(1) + Duration::seconds(30));
    }

    #[test]
    fn parse_rejects_invalid_designators_in_all_positions() {
        assert_eq!(
            "P1X".parse::<CalDuration>(),
            Err(ParseError::InvalidDuration("P1X".to_string()))
        );
        assert_eq!(
            "PT1D".parse::<CalDuration>(),
            Err(ParseError::InvalidDuration("PT1D".to_string()))
        );
        assert_eq!(
            "PT1H2H".parse::<CalDuration>(),
            Err(ParseError::InvalidDuration("PT1H2H".to_string()))
        );
        assert_eq!(
            "PT1H2M3M".parse::<CalDuration>(),
            Err(ParseError::InvalidDuration("PT1H2M3M".to_string()))
        );
    }

    #[test]
    fn parse_reports_invalid_number_overflow() {
        let err = "P18446744073709551616D".parse::<CalDuration>().unwrap_err();
        assert!(matches!(err, ParseError::InvalidNumber(_)));
    }
}
