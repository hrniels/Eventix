use anyhow::anyhow;
use std::{io::BufRead, str::FromStr};

use crate::parser::LineReader;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Property {
    name: String,
    params: Vec<Parameter>,
    value: String,
}

impl Property {
    pub fn new<N: ToString, V: ToString>(name: N, params: Vec<Parameter>, value: V) -> Self {
        Self {
            name: name.to_string(),
            params,
            value: value.to_string(),
        }
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn param(&self, name: &str) -> Option<&Parameter> {
        self.params.iter().find(|p| p.name() == name)
    }

    pub fn has_param_value(&self, name: &str, value: &str) -> bool {
        matches!(
            self.params.iter().find(|p| p.name() == name),
            Some(param) if param.value() == value
        )
    }

    pub fn params(&self) -> &[Parameter] {
        &self.params
    }

    pub fn value(&self) -> &String {
        &self.value
    }

    pub fn take_value(self) -> String {
        self.value
    }
}

impl FromStr for Property {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut name = String::new();
        let mut chars = s.char_indices();
        let (end, has_params) = loop {
            let Some((idx, c)) = chars.next() else {
                return Err(anyhow!("Missing name end"));
            };
            if c == ';' || c == ':' {
                break (idx + 1, c == ';');
            }
            name.push(c);
        };

        let (params, val_start) = if has_params {
            let mut in_quote = false;
            let mut param = String::new();
            let mut params = Vec::new();
            let end = loop {
                let Some((idx, c)) = chars.next() else {
                    return Err(anyhow!("Missing parameter end"));
                };
                if !in_quote {
                    if c == ';' || c == ':' {
                        params.push(param.parse::<Parameter>()?);
                        param.clear();
                        if c == ':' {
                            break idx + 1;
                        }
                    } else {
                        param.push(c);
                    }
                } else {
                    param.push(c);
                }
                if c == '"' {
                    in_quote = !in_quote;
                }
            };
            (params, end)
        } else {
            (vec![], end)
        };

        let value = s[val_start..].to_string();
        let value = value.replace(r"\n", "\n");
        let value = value.replace(r"\,", ",");
        let value = value.replace(r"\;", ";");
        let value = value.replace(r"\\", "\\");

        Ok(Self {
            name,
            params,
            value,
        })
    }
}

pub trait PropertyConsumer {
    fn from_lines<R: BufRead>(
        lines: &mut LineReader<R>,
        prop: Property,
    ) -> Result<Self, anyhow::Error>
    where
        Self: Sized;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Parameter {
    name: String,
    value: String,
}

impl Parameter {
    pub fn new(name: String, value: String) -> Self {
        Self { name, value }
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn value(&self) -> &String {
        &self.value
    }
}

impl FromStr for Parameter {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(2, '=');
        let name = parts
            .next()
            .ok_or_else(|| anyhow!("Missing parameter name"))?
            .to_string();
        let value = parts
            .next()
            .ok_or_else(|| anyhow!("Missing parameter value"))?;

        // strip quotes
        let value = if value.starts_with('"') {
            value[1..value.len() - 1].to_string()
        } else {
            value.to_string()
        };
        Ok(Self { name, value })
    }
}
