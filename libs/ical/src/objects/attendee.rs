// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{fmt, str::FromStr};

use crate::parser::{Parameter, ParseError, Property};

/// The participation role
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.2.16>.
#[derive(Default, Debug, Copy, Clone, Eq, PartialEq)]
pub enum CalRole {
    /// The participant chairs the event
    Chair,
    /// The attendance of the participant is required
    #[default]
    Required,
    /// The attendance of the participant is optional
    Optional,
    /// Participant will not attend; listed only for information purposes
    None,
}

impl fmt::Display for CalRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalRole::Chair => write!(f, "CHAIR"),
            CalRole::Required => write!(f, "REQ-PARTICIPANT"),
            CalRole::Optional => write!(f, "OPT-PARTICIPANT"),
            CalRole::None => write!(f, "NON-PARTICIPANT"),
        }
    }
}

impl FromStr for CalRole {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "CHAIR" => Ok(Self::Chair),
            "REQ-PARTICIPANT" => Ok(Self::Required),
            "OPT-PARTICIPANT" => Ok(Self::Optional),
            "NON-PARTICIPANT" => Ok(Self::None),
            _ => Err(ParseError::InvalidRole(s.to_string())),
        }
    }
}

/// The participation status for [`CalAttendee`].
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.2.12>.
#[derive(Default, Debug, Copy, Clone, Eq, PartialEq)]
pub enum CalPartStat {
    /// TODO/Event needs action
    #[default]
    NeedsAction,
    /// For events: the participant accepted the invitation
    Accepted,
    /// For events: the participant declined the invitation
    Declined,
    /// For events: the participant is undecided
    Tentative,
    /// For events: the participant delegated it to someone else
    Delegated,
    /// For TODOs: completed
    Completed,
    /// For TODOs: still in process
    InProcess,
}

impl fmt::Display for CalPartStat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalPartStat::NeedsAction => write!(f, "NEEDS-ACTION"),
            CalPartStat::Accepted => write!(f, "ACCEPTED"),
            CalPartStat::Declined => write!(f, "DECLINED"),
            CalPartStat::Tentative => write!(f, "TENTATIVE"),
            CalPartStat::Delegated => write!(f, "DELEGATED"),
            CalPartStat::Completed => write!(f, "COMPLETED"),
            CalPartStat::InProcess => write!(f, "IN-PROCESS"),
        }
    }
}

impl FromStr for CalPartStat {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "NEEDS-ACTION" => Ok(Self::NeedsAction),
            "ACCEPTED" => Ok(Self::Accepted),
            "DECLINED" => Ok(Self::Declined),
            "TENTATIVE" => Ok(Self::Tentative),
            "DELEGATED" => Ok(Self::Delegated),
            "COMPLETED" => Ok(Self::Completed),
            "IN-PROCESS" => Ok(Self::InProcess),
            _ => Err(ParseError::InvalidStatus(s.to_string())),
        }
    }
}

/// Represents an attendee in an ICalendar.
///
/// An attendee is identified by its address and optionally has a name, role, and participation
/// status.
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.4.1>.
#[derive(Default, Debug, Clone, Eq, PartialEq)]
pub struct CalAttendee {
    address: String,
    role: Option<CalRole>,
    part_stat: Option<CalPartStat>,
    common_name: Option<String>,
    params: Vec<Parameter>,
}

impl CalAttendee {
    /// Creates a new attendee with given address.
    pub fn new(address: String) -> Self {
        Self {
            address,
            ..Default::default()
        }
    }

    /// Returns the attendee's role.
    pub fn role(&self) -> Option<CalRole> {
        self.role
    }

    /// Sets the attendee's role.
    pub fn set_role(&mut self, role: CalRole) {
        self.role = Some(role);
    }

    /// Returns the participation status.
    pub fn part_stat(&self) -> Option<CalPartStat> {
        self.part_stat
    }

    /// Sets the participation status to given value.
    pub fn set_part_stat(&mut self, stat: Option<CalPartStat>) {
        self.part_stat = stat;
    }

    /// Returns the common name.
    pub fn common_name(&self) -> Option<&String> {
        self.common_name.as_ref()
    }

    /// Sets the common name.
    pub fn set_common_name(&mut self, cn: String) {
        self.common_name = Some(cn);
    }

    /// Returns the address with the "mailto:" prefix removed.
    ///
    /// Note that this method keeps the capitalization as it is. See [`Self::address`] if you need
    /// the address for comparisons.
    pub fn org_address(&self) -> &str {
        match self.address.strip_prefix("mailto:") {
            Some(addr) => addr,
            None => &self.address,
        }
    }

    /// Returns the address with the "mailto:" prefix removed and in lower case.
    pub fn address(&self) -> String {
        self.org_address().to_lowercase()
    }

    /// Returns a pretty name for this attendee.
    ///
    /// If the name is known, the pretty name is returned '$name <$address>'. Otherwise, only the
    /// address is returned.
    pub fn pretty_name(&self) -> String {
        let address = self.org_address();
        if let Some(name) = &self.common_name {
            format!("{name} <{address}>")
        } else {
            address.to_string()
        }
    }

    /// Builds and returns a [`Property`] for this attendee.
    pub fn to_prop(&self) -> Property {
        let mut params = Vec::new();
        if let Some(role) = &self.role {
            params.push(Parameter::new("ROLE", format!("{role}")));
        }
        if let Some(partstat) = &self.part_stat {
            params.push(Parameter::new("PARTSTAT", format!("{partstat}")));
        }
        if let Some(cn) = &self.common_name {
            params.push(Parameter::new("CN", cn.clone()));
        }
        params.extend(self.params.iter().cloned());
        Property::new("ATTENDEE", params, self.address.clone())
    }

    /// Merges the given attendee into `self`.
    ///
    /// The properties of the given attendee take preference, overwriting existing properties.
    pub fn merge_with(&mut self, att: CalAttendee) {
        if let Some(role) = att.role {
            self.role = Some(role);
        }
        if let Some(part_stat) = att.part_stat {
            self.part_stat = Some(part_stat);
        }
        if let Some(cn) = att.common_name {
            self.common_name = Some(cn);
        }
        for param in att.params {
            if let Some(ex_param) = self.params.iter_mut().find(|p| p.name() == param.name()) {
                *ex_param = param;
            } else {
                self.params.push(param);
            }
        }
    }
}

impl TryFrom<Property> for CalAttendee {
    type Error = ParseError;

    fn try_from(prop: Property) -> Result<Self, Self::Error> {
        let mut att = CalAttendee::default();
        for param in prop.params() {
            match param.name().as_str() {
                "PARTSTAT" => att.part_stat = Some(param.value().parse()?),
                "ROLE" => att.role = Some(param.value().parse()?),
                "CN" => att.common_name = Some(param.value().clone()),
                _ => att.params.push(param.clone()),
            }
        }
        att.address = prop.take_value();
        Ok(att)
    }
}

#[cfg(test)]
mod tests {
    use crate::parser::LineReader;

    use super::*;

    #[test]
    fn basics() {
        let att_str = "ATTENDEE;ROLE=CHAIR:mailto:mrbig@example.com";
        let line = LineReader::new(att_str.as_bytes()).next().unwrap();
        let prop = line.parse::<Property>().unwrap();
        let att = CalAttendee::try_from(prop).unwrap();
        assert_eq!(att.address, "mailto:mrbig@example.com");
        assert_eq!(att.role, Some(CalRole::Chair));
    }

    #[test]
    fn more_props() {
        let att_str = "ATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=TENTATIVE;CN=Henry
  Cabot:mailto:hcabot@example.com";
        let line = LineReader::new(att_str.as_bytes()).next().unwrap();
        let prop = line.parse::<Property>().unwrap();
        let att = CalAttendee::try_from(prop).unwrap();
        assert_eq!(att.address, "mailto:hcabot@example.com");
        assert_eq!(att.common_name, Some("Henry Cabot".to_string()));
        assert_eq!(att.part_stat, Some(CalPartStat::Tentative));
        assert_eq!(att.role, Some(CalRole::Required));
    }

    #[test]
    fn cal_role_round_trip() {
        let variants = [
            (CalRole::Chair, "CHAIR"),
            (CalRole::Required, "REQ-PARTICIPANT"),
            (CalRole::Optional, "OPT-PARTICIPANT"),
            (CalRole::None, "NON-PARTICIPANT"),
        ];
        for (role, expected_str) in variants {
            assert_eq!(format!("{role}"), expected_str);
            let parsed: CalRole = expected_str.parse().unwrap();
            assert_eq!(parsed, role);
        }
    }

    #[test]
    fn cal_role_invalid() {
        let result: Result<CalRole, _> = "INVALID".parse();
        assert!(result.is_err());
    }

    #[test]
    fn cal_part_stat_round_trip() {
        let variants = [
            (CalPartStat::NeedsAction, "NEEDS-ACTION"),
            (CalPartStat::Accepted, "ACCEPTED"),
            (CalPartStat::Declined, "DECLINED"),
            (CalPartStat::Tentative, "TENTATIVE"),
            (CalPartStat::Delegated, "DELEGATED"),
            (CalPartStat::Completed, "COMPLETED"),
            (CalPartStat::InProcess, "IN-PROCESS"),
        ];
        for (stat, expected_str) in variants {
            assert_eq!(format!("{stat}"), expected_str);
            let parsed: CalPartStat = expected_str.parse().unwrap();
            assert_eq!(parsed, stat);
        }
    }

    #[test]
    fn cal_part_stat_invalid() {
        let result: Result<CalPartStat, _> = "INVALID".parse();
        assert!(result.is_err());
    }

    #[test]
    fn attendee_new() {
        let att = CalAttendee::new("mailto:test@example.com".to_string());
        assert_eq!(att.address, "mailto:test@example.com");
        assert_eq!(att.role(), None);
        assert_eq!(att.part_stat(), None);
        assert_eq!(att.common_name(), None);
    }

    #[test]
    fn attendee_setters_getters() {
        let mut att = CalAttendee::new("mailto:test@example.com".to_string());

        assert_eq!(att.role(), None);
        att.set_role(CalRole::Optional);
        assert_eq!(att.role(), Some(CalRole::Optional));

        assert_eq!(att.part_stat(), None);
        att.set_part_stat(Some(CalPartStat::Accepted));
        assert_eq!(att.part_stat(), Some(CalPartStat::Accepted));
        att.set_part_stat(None);
        assert_eq!(att.part_stat(), None);

        assert_eq!(att.common_name(), None);
        att.set_common_name("John Doe".to_string());
        assert_eq!(att.common_name(), Some(&"John Doe".to_string()));
    }

    #[test]
    fn attendee_to_prop_with_all_fields() {
        let mut att = CalAttendee::new("mailto:test@example.com".to_string());
        att.set_role(CalRole::Chair);
        att.set_part_stat(Some(CalPartStat::Accepted));
        att.set_common_name("John Doe".to_string());

        let prop = att.to_prop();
        assert_eq!(prop.name(), "ATTENDEE");
        assert_eq!(prop.value(), "mailto:test@example.com");

        let params: Vec<_> = prop.params().to_vec();
        assert_eq!(params.len(), 3);
        assert!(
            params
                .iter()
                .any(|p| p.name() == "ROLE" && p.value() == "CHAIR")
        );
        assert!(
            params
                .iter()
                .any(|p| p.name() == "PARTSTAT" && p.value() == "ACCEPTED")
        );
        assert!(
            params
                .iter()
                .any(|p| p.name() == "CN" && p.value() == "John Doe")
        );
    }

    #[test]
    fn attendee_to_prop_with_only_role() {
        let mut att = CalAttendee::new("mailto:test@example.com".to_string());
        att.set_role(CalRole::Required);

        let prop = att.to_prop();
        let params: Vec<_> = prop.params().to_vec();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name(), "ROLE");
        assert_eq!(params[0].value(), "REQ-PARTICIPANT");
    }

    #[test]
    fn attendee_to_prop_with_only_part_stat() {
        let mut att = CalAttendee::new("mailto:test@example.com".to_string());
        att.set_part_stat(Some(CalPartStat::Declined));

        let prop = att.to_prop();
        let params: Vec<_> = prop.params().to_vec();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name(), "PARTSTAT");
        assert_eq!(params[0].value(), "DECLINED");
    }

    #[test]
    fn attendee_to_prop_with_only_common_name() {
        let mut att = CalAttendee::new("mailto:test@example.com".to_string());
        att.set_common_name("John Doe".to_string());

        let prop = att.to_prop();
        let params: Vec<_> = prop.params().to_vec();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name(), "CN");
        assert_eq!(params[0].value(), "John Doe");
    }

    #[test]
    fn attendee_org_address_without_mailto() {
        let att = CalAttendee::new("test@example.com".to_string());
        assert_eq!(att.org_address(), "test@example.com");
    }

    #[test]
    fn attendee_address_lowercase() {
        let att = CalAttendee::new("mailto:Test.Example@Example.COM".to_string());
        assert_eq!(att.address(), "test.example@example.com");
    }

    #[test]
    fn attendee_pretty_name_with_common_name() {
        let mut att = CalAttendee::new("mailto:test@example.com".to_string());
        att.set_common_name("John Doe".to_string());
        assert_eq!(att.pretty_name(), "John Doe <test@example.com>");
    }

    #[test]
    fn attendee_pretty_name_without_common_name() {
        let att = CalAttendee::new("mailto:test@example.com".to_string());
        assert_eq!(att.pretty_name(), "test@example.com");
    }

    #[test]
    fn attendee_merge_with_overwrites_existing() {
        let mut att1 = CalAttendee::new("mailto:test@example.com".to_string());
        att1.set_role(CalRole::Required);
        att1.set_part_stat(Some(CalPartStat::NeedsAction));
        att1.set_common_name("Old Name".to_string());

        let mut att2 = CalAttendee::new("mailto:test@example.com".to_string());
        att2.set_role(CalRole::Chair);
        att2.set_part_stat(Some(CalPartStat::Accepted));
        att2.set_common_name("New Name".to_string());

        att1.merge_with(att2);

        assert_eq!(att1.role(), Some(CalRole::Chair));
        assert_eq!(att1.part_stat(), Some(CalPartStat::Accepted));
        assert_eq!(att1.common_name(), Some(&"New Name".to_string()));
    }

    #[test]
    fn attendee_merge_with_keeps_existing_when_other_is_none() {
        let mut att1 = CalAttendee::new("mailto:test@example.com".to_string());
        att1.set_role(CalRole::Required);
        att1.set_part_stat(Some(CalPartStat::Accepted));
        att1.set_common_name("Name".to_string());

        let att2 = CalAttendee::new("mailto:other@example.com".to_string());

        att1.merge_with(att2);

        assert_eq!(att1.role(), Some(CalRole::Required));
        assert_eq!(att1.part_stat(), Some(CalPartStat::Accepted));
        assert_eq!(att1.common_name(), Some(&"Name".to_string()));
    }

    #[test]
    fn attendee_from_str_case_insensitive() {
        assert_eq!("chair".parse::<CalRole>().unwrap(), CalRole::Chair);
        assert_eq!("Chair".parse::<CalRole>().unwrap(), CalRole::Chair);
        assert_eq!(
            "accepted".parse::<CalPartStat>().unwrap(),
            CalPartStat::Accepted
        );
        assert_eq!(
            "Accepted".parse::<CalPartStat>().unwrap(),
            CalPartStat::Accepted
        );
    }

    #[test]
    fn attendee_try_from_with_unknown_param() {
        let att_str = "ATTENDEE;X-CUSTOM=value;PARTSTAT=ACCEPTED:mailto:test@example.com";
        let line = LineReader::new(att_str.as_bytes()).next().unwrap();
        let prop = line.parse::<Property>().unwrap();
        let att = CalAttendee::try_from(prop).unwrap();
        assert_eq!(att.address, "mailto:test@example.com");
        assert_eq!(att.part_stat, Some(CalPartStat::Accepted));
    }

    #[test]
    fn attendee_merge_with_params_adds_new() {
        let att_str1 = "ATTENDEE;X-OLD=oldvalue:mailto:test@example.com";
        let line1 = LineReader::new(att_str1.as_bytes()).next().unwrap();
        let prop1 = line1.parse::<Property>().unwrap();
        let mut att1 = CalAttendee::try_from(prop1).unwrap();

        let att_str2 = "ATTENDEE;X-NEW=newvalue:mailto:test@example.com";
        let line2 = LineReader::new(att_str2.as_bytes()).next().unwrap();
        let prop2 = line2.parse::<Property>().unwrap();
        let att2 = CalAttendee::try_from(prop2).unwrap();

        att1.merge_with(att2);

        let prop = att1.to_prop();
        let params: Vec<_> = prop.params().to_vec();
        assert_eq!(params.len(), 2);
        let names: Vec<_> = params.iter().map(|p| p.name().as_str()).collect();
        assert!(names.contains(&"X-OLD"));
        assert!(names.contains(&"X-NEW"));
    }

    #[test]
    fn attendee_merge_with_params_overwrites_existing() {
        let att_str1 = "ATTENDEE;X-KEY=oldvalue:mailto:test@example.com";
        let line1 = LineReader::new(att_str1.as_bytes()).next().unwrap();
        let prop1 = line1.parse::<Property>().unwrap();
        let mut att1 = CalAttendee::try_from(prop1).unwrap();

        let att_str2 = "ATTENDEE;X-KEY=newvalue:mailto:test@example.com";
        let line2 = LineReader::new(att_str2.as_bytes()).next().unwrap();
        let prop2 = line2.parse::<Property>().unwrap();
        let att2 = CalAttendee::try_from(prop2).unwrap();

        att1.merge_with(att2);

        let prop = att1.to_prop();
        let params: Vec<_> = prop.params().to_vec();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name(), "X-KEY");
        assert_eq!(params[0].value(), "newvalue");
    }
}
