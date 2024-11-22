mod line;
mod prop;

use std::num::ParseIntError;

use thiserror::Error;

pub use self::line::{LineReader, LineWriter};
pub use self::prop::{Parameter, Property, PropertyConsumer, PropertyProducer};

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
}

impl From<ParseIntError> for ParseError {
    fn from(err: ParseIntError) -> Self {
        ParseError::InvalidNumber(err)
    }
}
