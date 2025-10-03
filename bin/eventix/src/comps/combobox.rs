use askama::Template;
use eventix_locale::Locale;
use std::fmt::Display;
use std::sync::Arc;
use strum::IntoEnumIterator;

use crate::html::filters;

pub trait Named {
    fn name(&self) -> String;
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
    id: String,
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
            T::iter().map(|e| ComboOption::new(e.name(), e)).collect(),
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
        let name = name.to_string();
        Self {
            locale,
            id: name.replace("[", "_").replace("]", "_"),
            name,
            value,
            options,
        }
    }
}
