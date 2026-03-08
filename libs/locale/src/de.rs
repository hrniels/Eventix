use std::{io, path::Path};

use chrono::NaiveDate;
use chrono_tz::Tz;
use eventix_ical::objects::CalLocale;

use crate::{DateFlags, DateLike, LocaleType, Translations};

use super::Locale;

#[derive(Default, Debug)]
pub struct LocaleDe {
    tz: Tz,
    trans: Translations,
}

impl LocaleDe {
    pub fn new(tz: Tz, path: &Path) -> io::Result<Self> {
        let trans = Translations::new_from_file(path)?;
        Ok(Self { tz, trans })
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

    fn timezone(&self) -> &Tz {
        &self.tz
    }

    fn nth_day(&self, nth: u64, start: bool) -> String {
        match start {
            true => {
                format!("{nth}er")
            }
            false => {
                if nth == 1 {
                    String::from("Letzter")
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

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use chrono_tz::Tz;
    use eventix_ical::objects::CalLocale;

    use crate::{DateFlags, LocaleType};

    use super::{Locale, LocaleDe};

    fn fixed_date() -> NaiveDate {
        // 2024-03-21 is a Thursday / Donnerstag
        NaiveDate::from_ymd_opt(2024, 3, 21).unwrap()
    }

    // --- ty and timezone ---

    #[test]
    fn ty_and_timezone() {
        let l = LocaleDe::default();
        assert_eq!(l.ty(), LocaleType::German);
        assert_eq!(*l.timezone(), Tz::UTC);
    }

    // --- translate (no translations file → passthrough) ---

    #[test]
    fn translate_unknown_key_returns_key() {
        let l = LocaleDe::default();
        assert_eq!(l.translate("Montag"), "Montag");
    }

    // --- nth_day ---

    #[test]
    fn nth_day_start() {
        let l = LocaleDe::default();
        assert_eq!(l.nth_day(1, true), "1er");
        assert_eq!(l.nth_day(2, true), "2er");
        assert_eq!(l.nth_day(10, true), "10er");
    }

    #[test]
    fn nth_day_end_last() {
        let l = LocaleDe::default();
        assert_eq!(l.nth_day(1, false), "Letzter");
        assert_eq!(l.nth_day(2, false), "2t letzter");
        assert_eq!(l.nth_day(3, false), "3t letzter");
    }

    // --- fmt_naive_date ---

    #[test]
    fn fmt_naive_date_formats_short() {
        let l = LocaleDe::default();
        let d = fixed_date();
        // fmt_naive_date → fmt_date(Short) → without weekday prefix, month name untranslated.
        // Format: "<translated-month> <day>, <year>"
        // With no translation table, month stays in English ("Mar").
        let result = l.fmt_naive_date(&d);
        assert_eq!(result, "Mar 21, 2024");
    }

    // --- fmt_date (German override) ---

    #[test]
    fn fmt_date_short_no_today() {
        let l = LocaleDe::default();
        let d = fixed_date();
        // Short: no weekday prefix, month abbrev (untranslated = "Mar")
        let result = l.fmt_date(&d, DateFlags::Short | DateFlags::NoToday);
        assert_eq!(result, "Mar 21, 2024");
    }

    #[test]
    fn fmt_date_long_no_today() {
        let l = LocaleDe::default();
        let d = fixed_date();
        // Long: "<weekday>, <month-full> <day>, <year>" – weekday/month untranslated
        let result = l.fmt_date(&d, DateFlags::NoToday);
        assert_eq!(result, "Thursday, March 21, 2024");
    }

    // --- fmt_weekdate (German override) ---

    #[test]
    fn fmt_weekdate_short_no_today() {
        let l = LocaleDe::default();
        let d = fixed_date();
        // Short: "<abbrev-weekday>, <month-abbrev> <day>"
        // With no translations: weekday and month stay in English form.
        let result = l.fmt_weekdate(&d, DateFlags::Short | DateFlags::NoToday);
        assert_eq!(result, "Thu, Mar 21");
    }

    #[test]
    fn fmt_weekdate_long_no_today() {
        let l = LocaleDe::default();
        let d = fixed_date();
        // Long: "<full-weekday>, <month-abbrev> <day> <year>"
        let result = l.fmt_weekdate(&d, DateFlags::NoToday);
        assert_eq!(result, "Thursday, Mar 21 2024");
    }
}
