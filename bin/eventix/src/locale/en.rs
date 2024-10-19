use chrono::NaiveDate;
use numfmt::{Formatter, Precision};

use super::Locale;

#[derive(Default)]
pub struct LocaleEn {}

impl Locale for LocaleEn {
    fn translate<'a>(&self, key: &'a str) -> &'a str {
        key
    }

    fn format_currency(&self, num: f64) -> String {
        let mut f = Formatter::new()
            .separator(',')
            .unwrap()
            .precision(Precision::Decimals(2));
        let res = f.fmt2(num);
        // we want to have exactly 2 fraction digits
        match res.rfind('.') {
            Some(pos) if pos + 2 >= res.len() => format!("{}0", res),
            _ => res.to_string(),
        }
    }

    fn format_date(&self, date: &NaiveDate) -> String {
        format!("{}", date.format("%m/%d/%Y"))
    }
}
