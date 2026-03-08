use std::{io, path::Path};

use chrono::NaiveDate;
use chrono_tz::Tz;
use eventix_ical::objects::{CalLocale, CalLocaleEn};

use crate::{DateFlags, LocaleType, Translations};

use super::Locale;

#[derive(Default, Debug)]
pub struct LocaleEn {
    tz: Tz,
    trans: Translations,
}

impl LocaleEn {
    pub fn new(tz: Tz, path: &Path) -> io::Result<Self> {
        let trans = Translations::new_from_file(path)?;
        Ok(Self { tz, trans })
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

    fn timezone(&self) -> &Tz {
        &self.tz
    }

    fn nth_day(&self, nth: u64, start: bool) -> String {
        CalLocaleEn::nth_day(&CalLocaleEn, nth, start)
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

    use super::{Locale, LocaleEn};

    fn fixed_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 3, 21).unwrap()
    }

    // --- ty and timezone ---

    #[test]
    fn ty_and_timezone() {
        let l = LocaleEn::default();
        assert_eq!(l.ty(), LocaleType::English);
        assert_eq!(*l.timezone(), Tz::UTC);
    }

    // --- translate ---

    #[test]
    fn translate_unknown_key_returns_key() {
        let l = LocaleEn::default();
        assert_eq!(l.translate("SomeUnknownKey"), "SomeUnknownKey");
    }

    // --- nth_day ---

    #[test]
    fn nth_day_start_ordinals() {
        let l = LocaleEn::default();
        assert_eq!(l.nth_day(1, true), "1st");
        assert_eq!(l.nth_day(2, true), "2nd");
        assert_eq!(l.nth_day(3, true), "3rd");
        assert_eq!(l.nth_day(4, true), "4th");
        assert_eq!(l.nth_day(11, true), "11th");
        assert_eq!(l.nth_day(12, true), "12th");
        assert_eq!(l.nth_day(13, true), "13th");
        assert_eq!(l.nth_day(21, true), "21st");
    }

    #[test]
    fn nth_day_end_last() {
        let l = LocaleEn::default();
        assert_eq!(l.nth_day(1, false), "last");
        assert_eq!(l.nth_day(2, false), "2nd to last");
        assert_eq!(l.nth_day(3, false), "3rd to last");
        assert_eq!(l.nth_day(4, false), "4th to last");
    }

    // --- fmt_naive_date ---

    #[test]
    fn fmt_naive_date_formats_short() {
        let l = LocaleEn::default();
        let d = fixed_date();
        // fmt_naive_date calls fmt_date with DateFlags::Short → "%b %d, %Y"
        // The date is not today so it will not return a relative label.
        let result = l.fmt_naive_date(&d);
        assert_eq!(result, "Mar 21, 2024");
    }

    // --- fmt_date via Locale trait (NoToday to avoid flakiness) ---

    #[test]
    fn fmt_date_short() {
        let l = LocaleEn::default();
        let d = fixed_date();
        assert_eq!(
            l.fmt_date(&d, DateFlags::Short | DateFlags::NoToday),
            "Mar 21, 2024"
        );
    }

    #[test]
    fn fmt_date_long() {
        let l = LocaleEn::default();
        let d = fixed_date();
        assert_eq!(
            l.fmt_date(&d, DateFlags::NoToday),
            "Thursday, March 21, 2024"
        );
    }
}
