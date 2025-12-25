use std::{io, path::Path};

use chrono::NaiveDate;
use eventix_ical::objects::{CalLocale, CalLocaleEn};

use crate::{DateFlags, LocaleType, Translations};

use super::Locale;

#[derive(Default, Debug)]
pub struct LocaleEn {
    trans: Translations,
}

impl LocaleEn {
    pub fn new(path: &Path) -> io::Result<Self> {
        let trans = Translations::new_from_file(path)?;
        Ok(Self { trans })
    }
}

impl Locale for LocaleEn {
    fn ty(&self) -> LocaleType {
        LocaleType::English
    }

    fn translations(&self) -> &Translations {
        &self.trans
    }
}

impl CalLocale for LocaleEn {
    fn translate<'a>(&'a self, key: &'a str) -> &'a str {
        self.translations().table.get(key).map_or(key, |v| v)
    }

    fn timezone(&self) -> &chrono_tz::Tz {
        &chrono_tz::Europe::Berlin
    }

    fn nth_day(&self, nth: u64, start: bool) -> String {
        CalLocaleEn::nth_day(&CalLocaleEn, nth, start)
    }

    fn fmt_naive_date(&self, date: &NaiveDate) -> String {
        self.fmt_date(date, DateFlags::Short)
    }
}
