use anyhow::anyhow;
use std::{fmt::Display, str::FromStr};

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CalTodoStatus {
    NeedsAction,
    Completed,
    InProcess,
    Cancelled,
}

impl FromStr for CalTodoStatus {
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

impl Display for CalTodoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CalEventStatus {
    Tentative,
    Confirmed,
    Cancelled,
}

impl FromStr for CalEventStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "TENTATIVE" => Ok(Self::Tentative),
            "CANCELLED" => Ok(Self::Cancelled),
            "CONFIRMED" => Ok(Self::Confirmed),
            _ => Err(anyhow!("Invalid status {}", s)),
        }
    }
}

impl Display for CalEventStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
