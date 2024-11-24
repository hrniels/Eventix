use askama::Template;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};
use std::sync::Arc;
use strum::IntoEnumIterator;

use crate::html::filters;
use crate::locale::Locale;

pub trait Named {
    fn name(&self) -> String;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ComboValue<T: Display> {
    value: T,
}

pub struct ComboOption<T: Display> {
    name: String,
    value: T,
}

impl<T: Display> ComboOption<T> {
    pub fn new<N: ToString>(name: N, value: T) -> Self {
        Self {
            name: name.to_string(),
            value,
        }
    }
}

#[derive(Template)]
#[template(path = "comps/combobox.htm")]
pub struct ComboboxTemplate<T: Display + Eq + PartialEq> {
    name: String,
    value: Option<T>,
    options: Vec<ComboOption<T>>,
    locale: Arc<dyn Locale + Send + Sync>,
}

impl<T: Display + Eq + PartialEq + Named + IntoEnumIterator> ComboboxTemplate<T> {
    pub fn new<N: ToString>(
        locale: Arc<dyn Locale + Send + Sync>,
        name: N,
        value: Option<T>,
    ) -> Self {
        Self::new_with_options(
            locale,
            name,
            value,
            T::iter()
                .map(|e| ComboOption::new(format!("{}", e.name()), e))
                .collect(),
        )
    }
}

impl<T: Display + Eq + PartialEq> ComboboxTemplate<T> {
    pub fn new_with_options<N: ToString>(
        locale: Arc<dyn Locale + Send + Sync>,
        name: N,
        value: Option<T>,
        options: Vec<ComboOption<T>>,
    ) -> Self {
        Self {
            locale,
            name: name.to_string(),
            value,
            options,
        }
    }
}
