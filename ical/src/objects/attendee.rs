use std::str::FromStr;

use anyhow::anyhow;

use crate::parser::{Parameter, Property};

#[derive(Default, Debug, Copy, Clone, Eq, PartialEq)]
pub enum CalRole {
    Chair,
    #[default]
    Required,
    Optional,
    None,
}

impl FromStr for CalRole {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "CHAIR" => Ok(Self::Chair),
            "REQ-PARTICIPANT" => Ok(Self::Required),
            "OPT-PARTICIPANT" => Ok(Self::Optional),
            "NON-PARTICIPANT" => Ok(Self::None),
            _ => Err(anyhow!("Invalid role {}", s)),
        }
    }
}

#[derive(Default, Debug, Copy, Clone, Eq, PartialEq)]
pub enum CalPartStat {
    #[default]
    NeedsAction,
    Accepted,
    Declined,
    Tentative,
    Delegated,
    Completed,
    InProcess,
}

impl FromStr for CalPartStat {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "NEEDS-ACTION" => Ok(Self::NeedsAction),
            "ACCEPTED" => Ok(Self::Accepted),
            "DECLINED" => Ok(Self::Declined),
            "TENTATIVE" => Ok(Self::Tentative),
            "DELEGATED" => Ok(Self::Delegated),
            "COMPLETED" => Ok(Self::Completed),
            "IN-PROCESS" => Ok(Self::InProcess),
            _ => Err(anyhow!("Invalid participation status {}", s)),
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct CalAttendee {
    address: String,
    role: Option<CalRole>,
    part_stat: Option<CalPartStat>,
    common_name: Option<String>,
    params: Vec<Parameter>,
}

impl CalAttendee {
    pub fn address(&self) -> &String {
        &self.address
    }

    pub fn role(&self) -> Option<CalRole> {
        self.role
    }

    pub fn part_stat(&self) -> Option<CalPartStat> {
        self.part_stat
    }

    pub fn common_name(&self) -> Option<&String> {
        self.common_name.as_ref()
    }
}

impl TryFrom<Property> for CalAttendee {
    type Error = anyhow::Error;

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
