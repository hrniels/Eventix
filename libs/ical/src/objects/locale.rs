use chrono::NaiveDate;
use chrono_tz::Tz;
use std::fmt::{Debug, Write};

pub trait CalLocale: Debug {
    fn translate<'a>(&'a self, key: &'a str) -> &'a str;
    fn timezone(&self) -> &Tz;
    fn nth_day(&self, nth: u64, start: bool) -> String;
    fn fmt_naive_date(&self, date: &NaiveDate) -> String;
}

#[derive(Debug)]
pub struct CalLocaleEn;

impl CalLocaleEn {
    fn nth(&self, n: u64) -> String {
        let mut res = String::new();
        res.write_fmt(format_args!("{n}")).unwrap();
        match n % 10 {
            1 if n != 11 => res.push_str("st"),
            2 if n != 12 => res.push_str("nd"),
            3 if n != 13 => res.push_str("rd"),
            _ => res.push_str("th"),
        }
        res
    }
}

impl CalLocale for CalLocaleEn {
    fn translate<'a>(&'a self, key: &'a str) -> &'a str {
        key
    }

    fn timezone(&self) -> &Tz {
        &chrono_tz::UTC
    }

    fn nth_day(&self, nth: u64, start: bool) -> String {
        match start {
            true => self.nth(nth),
            false => {
                if nth == 1 {
                    String::from("last")
                } else {
                    format!("{} to last", self.nth(nth))
                }
            }
        }
    }

    fn fmt_naive_date(&self, date: &NaiveDate) -> String {
        date.format("%B %d, %Y").to_string()
    }
}
