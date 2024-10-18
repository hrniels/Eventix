use anyhow::anyhow;
use std::str::FromStr;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Status {
    NeedsAction,
    Completed,
    InProcess,
    Cancelled,
}

impl FromStr for Status {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "NEEDS-ACTION" => Ok(Self::NeedsAction),
            "COMPLETED" => Ok(Self::Completed),
            "IN-PROCESS" => Ok(Self::InProcess),
            "CANCELLED" => Ok(Self::Cancelled),
            _ => Err(anyhow!("Invalid status {}", s)),
        }
    }
}
