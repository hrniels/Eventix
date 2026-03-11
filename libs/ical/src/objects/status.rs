// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::parser::ParseError;

/// Represents the status of a TODO item.
///
/// This enum implements [`Display`](fmt::Display) and [`FromStr`] to convert to and from its
/// string representation.
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.1.11>.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CalTodoStatus {
    /// The TODO item is not done; action is required.
    NeedsAction,

    /// The TODO item has been completed.
    Completed,

    /// The TODO item has been started, but is not complete.
    InProcess,

    /// The TODO item has been canceled.
    Cancelled,
}

impl Serialize for CalTodoStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_some(&format!("{self}"))
    }
}

impl<'de> Deserialize<'de> for CalTodoStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let buf = String::deserialize(deserializer)?;
        CalTodoStatus::from_str(&buf).map_err(serde::de::Error::custom)
    }
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
        write!(f, "{s}")
    }
}

/// Represents the status of an event item.
///
/// This enum implements [`Display`](fmt::Display) and [`FromStr`] to convert to and from its
/// string representation.
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.1.11>.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CalEventStatus {
    /// The event is tentative.
    Tentative,

    /// The event has been confirmed.
    Confirmed,

    /// The event has been canceled.
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
        write!(f, "{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParseError;

    #[test]
    fn todo_from_str_and_display_and_serde_roundtrip() {
        // FromStr should be case-insensitive
        let s = "needs-action";
        let status = CalTodoStatus::from_str(s).expect("parse should succeed");
        assert_eq!(status, CalTodoStatus::NeedsAction);

        // Display must produce the canonical uppercase-with-dash string
        assert_eq!(format!("{}", status), "NEEDS-ACTION");

        // Serde JSON round-trip: the serialized form is a JSON string containing the display value
        let ser = serde_json::to_string(&status).expect("serialize should succeed");
        assert_eq!(ser, "\"NEEDS-ACTION\"");

        let de: CalTodoStatus = serde_json::from_str(&ser).expect("deserialize should succeed");
        assert_eq!(de, CalTodoStatus::NeedsAction);
    }

    #[test]
    fn todo_from_str_all_variants_and_invalid() {
        // Test all valid textual representations map to their variants
        assert_eq!(
            CalTodoStatus::from_str("COMPLETED").unwrap(),
            CalTodoStatus::Completed
        );
        assert_eq!(
            CalTodoStatus::from_str("in-process").unwrap(),
            CalTodoStatus::InProcess
        );
        assert_eq!(
            CalTodoStatus::from_str("CANCELLED").unwrap(),
            CalTodoStatus::Cancelled
        );

        // Invalid status returns a specific ParseError::InvalidStatus
        let err = CalTodoStatus::from_str("not-a-status").unwrap_err();
        assert_eq!(err, ParseError::InvalidStatus("not-a-status".to_string()));
    }

    #[test]
    fn event_from_str_and_display_and_invalid() {
        // valid parsing and display
        let ev = CalEventStatus::from_str("tentative").expect("parse should succeed");
        assert_eq!(ev, CalEventStatus::Tentative);
        assert_eq!(format!("{}", ev), "TENTATIVE");

        let ev2 = CalEventStatus::from_str("CONFIRMED").expect("parse should succeed");
        assert_eq!(ev2, CalEventStatus::Confirmed);
        assert_eq!(format!("{}", ev2), "CONFIRMED");

        // invalid
        let err = CalEventStatus::from_str("foo").unwrap_err();
        assert_eq!(err, ParseError::InvalidStatus("foo".to_string()));
    }
}
