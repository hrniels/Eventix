use std::{fmt, str::FromStr};

use crate::parser::ParseError;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CalTodoStatus {
    NeedsAction,
    Completed,
    InProcess,
    Cancelled,
}

impl FromStr for CalTodoStatus {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "NEEDS-ACTION" => Ok(Self::NeedsAction),
            "COMPLETED" => Ok(Self::Completed),
            "IN-PROCESS" => Ok(Self::InProcess),
            "CANCELLED" => Ok(Self::Cancelled),
            _ => Err(ParseError::InvalidStatus(s.to_string())),
        }
    }
}

impl fmt::Display for CalTodoStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            CalTodoStatus::NeedsAction => "NEEDS-ACTION",
            CalTodoStatus::Completed => "COMPLETED",
            CalTodoStatus::InProcess => "IN-PROCESS",
            CalTodoStatus::Cancelled => "CANCELLED",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CalEventStatus {
    Tentative,
    Confirmed,
    Cancelled,
}

impl FromStr for CalEventStatus {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "TENTATIVE" => Ok(Self::Tentative),
            "CANCELLED" => Ok(Self::Cancelled),
            "CONFIRMED" => Ok(Self::Confirmed),
            _ => Err(ParseError::InvalidStatus(s.to_string())),
        }
    }
}

impl fmt::Display for CalEventStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            CalEventStatus::Tentative => "TENTATIVE",
            CalEventStatus::Confirmed => "CONFIRMED",
            CalEventStatus::Cancelled => "CANCELLED",
        };
        write!(f, "{}", s)
    }
}
