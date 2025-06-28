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
    pub fn address(&self) -> &str {
        match self.address.strip_prefix("mailto:") {
            Some(addr) => addr,
            None => &self.address,
        }
    }

    /// Returns a pretty name for this attendee.
    ///
    /// If the name is known, the pretty name is returned '$name <$address>'. Otherwise, only the
    /// address is returned.
    pub fn pretty_name(&self) -> String {
        let address = self.address();
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
}
