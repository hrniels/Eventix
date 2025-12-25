//! Abstractions for iCalendar objects.
//!
//! A iCalendar object is represented by [`Calendar`], which contains one or more _components_,
//! represented as [`CalComponent`]. Such a component is either a [`CalEvent`] or a [`CalTodo`].
//! Both share most of their properties, implemented by [`EventLikeComponent`], but have small
//! differences as well.
//!
//! The properties can be alarms ([`CalAlarm`]), attendees ([`CalAttendee`]), dates ([`CalDate`]),
//! organizers ([`CalOrganizer`]), recurrency rules ([`CalRRule`]), and a status
//! ([`CalEventStatus`] or [`CalTodoStatus`]).

mod alarm;
mod attendee;
mod calendar;
mod component;
mod date;
mod duration;
mod event;
mod evlike;
mod locale;
mod organizer;
mod recur;
mod status;
mod todo;

pub use alarm::{AlarmOverlay, CalAction, CalAlarm, CalRelated, CalTrigger, DefaultAlarmOverlay};
pub use attendee::{CalAttendee, CalPartStat, CalRole};
pub use calendar::{CalTimeZone, Calendar};
pub use component::{
    CalCompType, CalComponent, CompDateIterator, CompDateType, EventLikeComponent, PRIORITY_HIGH,
    PRIORITY_LOW, PRIORITY_MEDIUM,
};
pub use date::{CalDate, CalDateTime, CalDateType};
pub use duration::CalDuration;
pub use event::CalEvent;
pub use evlike::{EventLike, UpdatableEventLike};
pub use locale::{CalLocale, CalLocaleEn};
pub use organizer::CalOrganizer;
pub use recur::{CalRRule, CalRRuleFreq, CalRRuleSide, CalWDayDesc, WeekdayHuman};
pub use status::{CalEventStatus, CalTodoStatus};
pub use todo::CalTodo;
