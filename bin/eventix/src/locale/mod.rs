mod de;
mod en;

use chrono::DateTime;
pub use de::LocaleDe;
#[allow(unused_imports)]
pub use en::LocaleEn;

use chrono_tz::Tz;

use std::sync::Arc;

pub trait Locale {
    fn timezone(&self) -> &Tz {
        &chrono_tz::Europe::Berlin
    }

    fn format_date(&self, date: &DateTime<Tz>) -> String {
        date.format("%A, %B %d, %Y").to_string()
    }

    fn format_datetime(&self, date: &DateTime<Tz>) -> String {
        date.format("%A, %B %d, %Y %H:%M:%S").to_string()
    }

    fn translate<'a>(&self, key: &'a str) -> &'a str;
}

pub fn default() -> Arc<dyn Locale + Send + Sync> {
    Arc::new(LocaleDe::default())
}
