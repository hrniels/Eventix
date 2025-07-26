//! Abstractions for iCalendar objects according to RFC 5545.
//!
//! This crate provides means to parse iCalendar files into objects, perform operations on these
//! objects, and write them back to disk.
//!
//! At first, the [`parser`] module is responsible for parsing such files into objects such as
//! [`CalEvent`](objects::CalEvent), or [`CalAlarm`](objects::CalAlarm). The parser module also
//! hosts low-level routines such as [`LineWriter`](parser::LineWriter) to write such objects back
//! to disk.
//!
//! Second, the [`objects`] module hosts various iCalendar objects and offers operations to
//! manipulate them and determining event occurrences in a given time frame.
//!
//! Finally, the [`col`] module offers collections to manage these objects, starting with
//! [`CalFile`](col::CalFile) over [`CalDir`](col::CalDir) to [`CalStore`](col::CalStore).
//!
//! The RFC can be found at <https://datatracker.ietf.org/doc/html/rfc5545>.
//!
//! # Examples
//!
//! Parsing a calendar from file and inspecting its properties:
//!
//! ```
//! use eventix_ical::objects::{CalCompType, Calendar, EventLike};
//! use eventix_ical::parser::Property;
//!
//! let cal_str = "BEGIN:VCALENDAR
//! VERSION:2.0
//! BEGIN:VTODO
//! UID:1234-5678
//! SUMMARY:This is a test!
//! END:VTODO
//! END:VCALENDAR";
//!
//! // parse calendar and inspect properties
//! let cal = cal_str.parse::<Calendar>().unwrap();
//! assert_eq!(cal.properties()[0], Property::new("VERSION", vec![], "2.0"));
//!
//! // get its components (events/TODOs)
//! let comps = cal.components();
//!
//! // the first is a TODO; inspect its properties
//! assert_eq!(comps[0].ctype(), CalCompType::Todo);
//! let todo = comps[0].as_todo().unwrap();
//! assert_eq!(todo.uid().as_str(), "1234-5678");
//! assert_eq!(todo.summary(), Some(&"This is a test!".to_string()));
//! ```
//!
//! Iterate through occurrences of a TODO:
//!
//! ```
//! use chrono::{NaiveDate, TimeZone};
//! use eventix_ical::col::CalFile;
//! use eventix_ical::objects::{Calendar, EventLike};
//! use std::sync::Arc;
//!
//! let cal_str = "BEGIN:VCALENDAR
//! BEGIN:VTODO
//! UID:1234-5678
//! DTSTART:20241024T090000Z
//! RRULE:FREQ=DAILY;INTERVAL=2
//! SUMMARY:This is a test!
//! END:VTODO
//! END:VCALENDAR";
//!
//! // parse and create dummy CalFile
//! let cal = cal_str.parse::<Calendar>().unwrap();
//! let file = CalFile::new(Arc::new(String::from("")), "".into(), cal);
//!
//! // walk through occurrences
//! let tz = chrono_tz::Europe::Berlin;
//! let start = tz.with_ymd_and_hms(2024, 10, 1, 10, 0, 0).unwrap();
//! let end = tz.with_ymd_and_hms(2024, 12, 1, 10, 0, 0).unwrap();
//! let mut occs = file.occurrences_between(start, end, |_| true);
//!
//! // the first is on 2024-10-24
//! let next = occs.next().unwrap();
//! assert_eq!(
//!     next.occurrence_start().unwrap().date_naive(),
//!     NaiveDate::from_ymd_opt(2024, 10, 24).unwrap()
//! );
//!
//! let next = occs.next().unwrap();
//! assert_eq!(
//!     next.occurrence_start().unwrap().date_naive(),
//!     NaiveDate::from_ymd_opt(2024, 10, 26).unwrap()
//! );
//! // can also access the TODO's properties
//! assert_eq!(next.summary(), Some(&String::from("This is a test!")));
//! ```

pub mod col;
pub mod objects;
pub mod parser;
pub mod util;
