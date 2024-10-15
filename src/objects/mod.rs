use chrono::{DateTime, Local, TimeZone, Utc};
use chrono_tz::Tz;
use icalendar::{CalendarDateTime, DatePerhapsTime};
use once_cell::sync::Lazy;
use std::sync::Mutex;

mod item;
mod recur;
mod source;
mod store;

pub use item::CalItem;
pub use recur::{Frequency, RecurrenceRule};
pub use source::CalSource;
pub use store::CalStore;

pub type Id = u64;

pub fn generate_id() -> Id {
    static NEXT_ID: Lazy<Mutex<Id>> = Lazy::new(|| Mutex::new(0));
    let mut next = NEXT_ID.lock().unwrap();
    let res = *next + 1;
    *next += 1;
    res
}

pub fn ical_datetime_to_tz(ical: &CalendarDateTime, tz: &Tz) -> DateTime<Tz> {
    match ical {
        CalendarDateTime::Utc(dt) => dt.with_timezone(tz),
        CalendarDateTime::WithTimezone {
            date_time: dt,
            tzid,
        } => {
            let date_tz = if let Ok(date_tz) = tzid.parse::<Tz>() {
                date_tz
            } else {
                // we fall back to UTC for all weird values that we see
                Tz::UTC
            };
            date_tz.from_utc_datetime(&dt).with_timezone(tz)
        }
        CalendarDateTime::Floating(dt) => {
            let local = Local.from_utc_datetime(&dt);
            local.with_timezone(tz)
        }
    }
}

pub fn ical_date_to_tz(ical: &DatePerhapsTime, tz: &Tz) -> DateTime<Tz> {
    match ical {
        DatePerhapsTime::Date(date) => Utc
            .from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
            .with_timezone(tz),
        DatePerhapsTime::DateTime(datetime) => ical_datetime_to_tz(datetime, tz),
    }
}
