// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt::Display;

use crate::parser::{Parameter, ParseError, Property};

/// Represents an organizers of an event or TODO.
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.4.3>.
#[derive(Default, Debug, Clone, Eq, PartialEq)]
pub struct CalOrganizer {
    address: String,
    common_name: Option<String>,
    sent_by: Option<String>,
    params: Vec<Parameter>,
}

impl CalOrganizer {
    /// Creates a new organizer instance with `name` as the common name and given email address.
    pub fn new_named<T: ToString, S: Display>(name: T, address: S) -> Self {
        Self {
            address: format!("mailto:{address}"),
            common_name: Some(name.to_string()),
            sent_by: None,
            params: vec![],
        }
    }

    /// Returns the address with the "mailto:" prefix removed.
    pub fn address(&self) -> &str {
        match self.address.strip_prefix("mailto:") {
            Some(addr) => addr,
            None => &self.address,
        }
    }

    /// Returns the common name of the organizer.
    pub fn common_name(&self) -> Option<&String> {
        self.common_name.as_ref()
    }

    /// Returns the send-by address, if specified.
    ///
    /// If this property is specified, it denotes that this person acts on behalf of the organizer.
    pub fn sent_by(&self) -> Option<&String> {
        self.sent_by.as_ref()
    }

    /// Turns this organizer into a [`Property`].
    pub fn to_prop(&self) -> Property {
        let mut params = Vec::new();
        if let Some(cn) = &self.common_name {
            params.push(Parameter::new("CN", cn.clone()));
        }
        if let Some(sent_by) = &self.sent_by {
            params.push(Parameter::new("SENT-BY", sent_by.clone()));
        }
        params.extend(self.params.iter().cloned());
        Property::new("ORGANIZER", params, self.address.clone())
    }
}

impl TryFrom<Property> for CalOrganizer {
    type Error = ParseError;

    fn try_from(prop: Property) -> Result<Self, Self::Error> {
        let mut org = CalOrganizer::default();
        for param in prop.params() {
            match param.name().as_str() {
                "CN" => org.common_name = Some(param.value().clone()),
                "SENT-BY" => org.sent_by = Some(param.value().clone()),
                _ => org.params.push(param.clone()),
            }
        }
        org.address = prop.take_value();
        Ok(org)
    }
}

#[cfg(test)]
mod tests {
    use crate::parser::LineReader;
    use crate::parser::{Parameter, Property};

    use super::*;

    #[test]
    fn basics() {
        let att_str = "ORGANIZER;CN=John Smith:mailto:jsmith@example.com";
        let line = LineReader::new(att_str.as_bytes()).next().unwrap();
        let prop = line.parse::<Property>().unwrap();
        let org = CalOrganizer::try_from(prop).unwrap();
        assert_eq!(org.address, "mailto:jsmith@example.com");
        assert_eq!(org.common_name, Some("John Smith".to_string()));
    }

    #[test]
    fn more_props() {
        let att_str = "ORGANIZER;SENT-BY=\"mailto:jane_doe@example.com\":mailto:jsmith@example.com";
        let line = LineReader::new(att_str.as_bytes()).next().unwrap();
        let prop = line.parse::<Property>().unwrap();
        let org = CalOrganizer::try_from(prop).unwrap();
        assert_eq!(org.address, "mailto:jsmith@example.com");
        assert_eq!(org.sent_by, Some("mailto:jane_doe@example.com".to_string()));
    }

    #[test]
    fn new_named_and_to_prop_exact_format() {
        let org = CalOrganizer::new_named("John Smith", "jsmith@example.com");
        // internal value contains the mailto: prefix
        assert_eq!(org.address, "mailto:jsmith@example.com");
        // address() strips the mailto: prefix for callers
        assert_eq!(org.address(), "jsmith@example.com");
        assert_eq!(org.common_name(), Some(&"John Smith".to_string()));

        let prop = org.to_prop();
        // exact string representation (no extra params)
        assert_eq!(
            format!("{}", prop),
            "ORGANIZER;CN=John Smith:mailto:jsmith@example.com"
        );
    }

    #[test]
    fn to_prop_includes_sent_by_and_custom_params_and_quotes() {
        let org = CalOrganizer {
            address: "mailto:jsmith@example.com".to_string(),
            common_name: Some("John Smith".to_string()),
            sent_by: Some("mailto:jane_doe@example.com".to_string()),
            params: vec![Parameter::new("X-FOO", "bar")],
        };

        let prop = org.to_prop();
        // SENT-BY contains a ':' and therefore must be quoted by the parameter formatter
        let expected = "ORGANIZER;CN=John Smith;SENT-BY=\"mailto:jane_doe@example.com\";\
X-FOO=bar:mailto:jsmith@example.com";
        assert_eq!(format!("{}", prop), expected);
    }

    #[test]
    fn try_from_preserves_unknown_params_and_handles_non_mailto_address() {
        let prop = Property::new(
            "ORGANIZER",
            vec![Parameter::new("X-CUST", "Value")],
            "jsmith@example.com",
        );
        let org = CalOrganizer::try_from(prop).unwrap();
        // since the value did not start with mailto: the internal address keeps it as-is
        assert_eq!(org.address, "jsmith@example.com");
        assert_eq!(org.address(), "jsmith@example.com");
        // unknown param should have been pushed into params
        assert_eq!(org.params, vec![Parameter::new("X-CUST", "Value")]);
        assert_eq!(org.sent_by(), None);
    }
}
