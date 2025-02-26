//! Provides abstractions for the iCalendar objects.
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
mod event;
mod evlike;
mod organizer;
mod recur;
mod status;
mod todo;

pub use alarm::{CalAction, CalAlarm, CalRelated, CalTrigger};
pub use attendee::{CalAttendee, CalPartStat, CalRole};
pub use calendar::Calendar;
pub use component::{
    CalCompType, CalComponent, CompDateIterator, CompDateType, EventLikeComponent,
};
pub use date::{CalDate, CalDateTime, CalDateType};
pub use event::CalEvent;
pub use evlike::{EventLike, UpdatableEventLike};
pub use organizer::CalOrganizer;
pub use recur::{CalRRule, CalRRuleFreq, CalRRuleSide, CalWDayDesc, WeekdayHuman};
pub use status::{CalEventStatus, CalTodoStatus};
pub use todo::CalTodo;
