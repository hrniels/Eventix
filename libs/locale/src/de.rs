use std::{io, path::Path};

use chrono::NaiveDate;
use eventix_ical::objects::CalLocale;

use crate::{DateFlags, DateLike, LocaleType, Translations};

use super::Locale;

#[derive(Default, Debug)]
pub struct LocaleDe {
    trans: Translations,
}

impl LocaleDe {
    pub fn new(path: &Path) -> io::Result<Self> {
        let trans = Translations::new_from_file(path)?;
        Ok(Self { trans })
    }
}

impl Locale for LocaleDe {
    fn ty(&self) -> LocaleType {
        LocaleType::German
    }

    fn translations(&self) -> &Translations {
        &self.trans
    }

    fn fmt_weekdate(&self, date: &dyn DateLike, flags: DateFlags) -> String {
        if !flags.contains(DateFlags::NoToday)
            && let Some(rel) = self.has_relative(date)
        {
            return rel.to_string();
        }

        let wday = if flags.contains(DateFlags::Short) {
            "%a"
        } else {
            "%A"
        };
        let wday_en = date.fmt(wday);
        let wday = self.translate(&wday_en);

        let mon_en = date.fmt("%b");
        let mon = self.translate(&mon_en);

        let fmt = if flags.contains(DateFlags::Short) {
            "%d"
        } else {
            "%d %Y"
        };
        format!("{}, {} {}", wday, mon, date.fmt(fmt))
    }

    fn fmt_date(&self, date: &dyn DateLike, flags: DateFlags) -> String {
        if !flags.contains(DateFlags::NoToday)
            && let Some(rel) = self.has_relative(date)
        {
            return rel.to_string();
        }

        let wday = if !flags.contains(DateFlags::Short) {
            let wday_en = date.fmt("%A");
            format!("{}, ", self.translate(&wday_en))
        } else {
            String::new()
        };

        let mon_fmt = if flags.contains(DateFlags::Short) {
            "%b"
        } else {
            "%B"
        };
        let mon_en = date.fmt(mon_fmt);
        let mon = self.translate(&mon_en);

        let day_year = date.fmt("%d, %Y");
        format!("{}{} {}", wday, mon, day_year)
    }
}

impl CalLocale for LocaleDe {
    fn translate<'a>(&'a self, key: &'a str) -> &'a str {
        self.translations().table.get(key).map_or(key, |v| v)
    }

    fn timezone(&self) -> &chrono_tz::Tz {
        &chrono_tz::Europe::Berlin
    }

    fn nth_day(&self, nth: u64, start: bool) -> String {
        match start {
            true => {
                format!("{nth}er")
            }
            false => {
                if nth == 1 {
                    format!("Letzter")
                } else {
                    format!("{nth}t letzter")
                }
            }
        }
    }

    fn fmt_naive_date(&self, date: &NaiveDate) -> String {
        self.fmt_date(date, DateFlags::Short)
    }
}
