use std::{
    fmt::{self, Write},
    io::BufRead,
    str::FromStr,
};

use crate::parser::{LineReader, ParseError};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Property {
    name: String,
    params: Vec<Parameter>,
    value: String,
    escaped: bool,
}

impl Property {
    pub fn new<N: ToString, V: ToString>(name: N, params: Vec<Parameter>, value: V) -> Self {
        Self {
            name: name.to_string(),
            params,
            value: value.to_string(),
            escaped: false,
        }
    }

    pub fn new_escaped<N: ToString, V: ToString>(
        name: N,
        params: Vec<Parameter>,
        value: V,
    ) -> Self {
        Self {
            name: name.to_string(),
            params,
            value: value.to_string(),
            escaped: true,
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

impl fmt::Display for Property {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        for p in &self.params {
            write!(f, ";{}", p)?;
        }

        f.write_char(':')?;
        if self.escaped {
            write!(f, "{}", self.value)
        } else {
            for c in self.value.chars() {
                if c == ';' || c == ',' || c == '\n' {
                    f.write_char('\\')?;
                }
                // TODO that's incomplete
                match c {
                    '\n' => f.write_char('n')?,
                    c => f.write_char(c)?,
                }
            }
            Ok(())
        }
    }
}

impl FromStr for Property {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut name = String::new();
        let mut chars = s.char_indices();
        let (end, has_params) = loop {
            let Some((idx, c)) = chars.next() else {
                return Err(ParseError::MissingNameEnd);
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
                    return Err(ParseError::MissingParamEnd);
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
            // these are special cases, which do not use escaping
            escaped: name == "RRULE" || name == "CATEGORIES",
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
    ) -> Result<Self, ParseError>
    where
        Self: Sized;
}

pub trait PropertyProducer {
    fn to_props(&self) -> Vec<Property>;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Parameter {
    name: String,
    value: String,
}

impl Parameter {
    pub fn new<N: ToString, V: ToString>(name: N, value: V) -> Self {
        Self {
            name: name.to_string(),
            value: value.to_string(),
        }
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn value(&self) -> &String {
        &self.value
    }
}

impl fmt::Display for Parameter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}=", self.name)?;
        if self.value.contains([':', ';', ',']) {
            write!(f, "\"{}\"", self.value)?;
        } else {
            write!(f, "{}", self.value)?;
        }
        Ok(())
    }
}

impl FromStr for Parameter {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(2, '=');
        let name = parts.next().unwrap().to_string();
        let value = parts.next().ok_or_else(|| ParseError::MissingParamValue)?;

        // strip quotes
        let value = if value.starts_with('"') {
            value[1..value.len() - 1].to_string()
        } else {
            value.to_string()
        };
        Ok(Self { name, value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        let prop_str = "BEGIN:VCALENDAR";
        let prop = prop_str.parse::<Property>().unwrap();
        assert_eq!(prop.name(), "BEGIN");
        assert_eq!(prop.params(), []);
        assert_eq!(prop.value(), "VCALENDAR");
        assert_eq!(format!("{}", prop), prop_str);
    }

    #[test]
    fn errors() {
        assert_eq!("BEGIN".parse::<Property>(), Err(ParseError::MissingNameEnd));
        assert_eq!(
            "BEGIN;TEST".parse::<Property>(),
            Err(ParseError::MissingParamEnd)
        );
        assert_eq!(
            "BEGIN;:BLA".parse::<Property>(),
            Err(ParseError::MissingParamValue)
        );
    }

    #[test]
    fn param_with_quotes() {
        let prop_str = "DTSTART;TZID=\"My:TZ\":20241024T090000";
        let prop = prop_str.parse::<Property>().unwrap();
        assert_eq!(prop.name(), "DTSTART");
        assert_eq!(
            prop.params(),
            [Parameter::new("TZID".to_string(), "My:TZ".to_string())]
        );
        assert_eq!(prop.value(), "20241024T090000");
        assert_eq!(format!("{}", prop), prop_str);
    }

    #[test]
    fn value_with_quotes() {
        let prop_str = "TEST;FOO=bar;A=B:\"value\"";
        let prop = prop_str.parse::<Property>().unwrap();
        assert_eq!(prop.name(), "TEST");
        assert_eq!(
            prop.params(),
            [
                Parameter::new("FOO".to_string(), "bar".to_string()),
                Parameter::new("A".to_string(), "B".to_string())
            ]
        );
        assert_eq!(prop.value(), "\"value\"");
        assert_eq!(format!("{}", prop), prop_str);
    }

    #[test]
    fn with_escapes() {
        let prop_str = "SUMMARY:foo bar
 test with\\n
  multiple\\;\\,
  lines";
        let mut reader = LineReader::new(prop_str.as_bytes());
        let prop = reader.next().unwrap().parse::<Property>().unwrap();
        assert_eq!(prop.name(), "SUMMARY");
        assert_eq!(prop.params(), []);
        assert_eq!(
            prop.value(),
            r"foo bartest with
 multiple;, lines"
        );
        assert_eq!(
            format!("{}", prop),
            "SUMMARY:foo bartest with\\n multiple\\;\\, lines"
        );
    }
}
