use chrono::NaiveDate;
use numfmt::{Formatter, Precision};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::Locale;

#[derive(Default)]
pub struct LocaleDe {}

impl Locale for LocaleDe {
    fn translate<'a>(&self, key: &'a str) -> &'a str {
        static XLATE_TABLE: Lazy<HashMap<&str, &str>> = Lazy::new(|| {
            HashMap::from([
                ("Events", "Ereignisse"),
                ("Error", "Fehler"),
                ("Errors", "Fehler"),
                ("Information", "Information"),
                ("Yes", "Ja"),
                ("No", "Nein"),
                ("Page generation time", "Ladezeit"),
            ])
        });
        XLATE_TABLE.get(key).unwrap_or(&key)
    }

    fn format_currency(&self, num: f64) -> String {
        let mut f = Formatter::new()
            .separator('.')
            .unwrap()
            .precision(Precision::Decimals(2));
        let res = f.fmt2(num);
        // we want to have exactly 2 fraction digits
        match res.rfind(',') {
            Some(pos) if pos + 2 >= res.len() => format!("{}0", res),
            _ => res.to_string(),
        }
    }

    fn format_date(&self, date: &NaiveDate) -> String {
        format!("{}", date.format("%d.%m.%Y"))
    }
}
