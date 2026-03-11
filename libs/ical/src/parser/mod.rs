// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Parser utilities for RFC 5545.
//!
//! This module implements various abstractions to deal with the iCalendar format according to RFC
//! 5545.
//!
//! The lowest level provide the [`LineReader`] and [`LineWriter`] that allow their users to work
//! with logical lines, while parsing or producing physical lines as expected by the format (at
//! most 75 bytes per line).
//!
//! On top of them, a line can be turned into a [`Property`] (and a property back into a line),
//! having a name, value, and optionally [`Parameter`]s.
//!
//! Finally, [`PropertyConsumer`] and [`PropertyProducer`] provide means to parse multiple lines
//! into a recursive object structure and back into a vector of [`Property`]s.

mod line;
mod prop;

use std::num::ParseIntError;

use thiserror::Error;

pub use self::line::{LineReader, LineWriter};
pub use self::prop::{Parameter, Property, PropertyConsumer, PropertyProducer};

/// Errors that occur during parsing of iCalendar objects.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ParseError {
    #[error("Missing name end")]
    MissingNameEnd,
    #[error("Missing parameter end")]
    MissingParamEnd,
    #[error("Missing parameter value")]
    MissingParamValue,
    #[error("Unexpected property: {0}")]
    UnexpectedProp(String),
    #[error("Unexpected END:{0}")]
    UnexpectedEnd(String),
    #[error("Unexpected BEGIN:{0}")]
    UnexpectedBegin(String),
    #[error("Invalid weekday description end")]
    UnexpectedWDayEnd,
    #[error("Unexpected rrule {0}")]
    UnexpectedRRule(String),
    #[error("Unexpected end of file")]
    UnexpectedEOF,
    #[error("Invalid percentage: {0}")]
    InvalidPercent(u8),
    #[error("Invalid priority: {0}")]
    InvalidPriority(u8),
    #[error("Malformed date: {0}")]
    MalformedDate(String),
    #[error("Invalid date: {0}")]
    InvalidDate(String),
    #[error("Invalid number: {0}")]
    InvalidNumber(ParseIntError),
    #[error("Invalid status: {0}")]
    InvalidStatus(String),
    #[error("Invalid role: {0}")]
    InvalidRole(String),
    #[error("Invalid frequency: {0}")]
    InvalidFrequency(String),
    #[error("Invalid side: {0}")]
    InvalidSide(String),
    #[error("Invalid weekday: {0}")]
    InvalidWeekday(String),
    #[error("Invalid action: {0}")]
    InvalidAction(String),
    #[error("Invalid duration: {0}")]
    InvalidDuration(String),
}

impl From<ParseIntError> for ParseError {
    fn from(err: ParseIntError) -> Self {
        ParseError::InvalidNumber(err)
    }
}
