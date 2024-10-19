mod de;
mod en;

pub use de::LocaleDe;
#[allow(unused_imports)]
pub use en::LocaleEn;

use chrono::NaiveDate;

use std::sync::Arc;

pub trait Locale {
    fn translate<'a>(&self, key: &'a str) -> &'a str;
    fn format_currency(&self, num: f64) -> String;
    fn format_date(&self, date: &NaiveDate) -> String;
}

pub fn default() -> Arc<dyn Locale + Send + Sync> {
    Arc::new(LocaleDe::default())
}
