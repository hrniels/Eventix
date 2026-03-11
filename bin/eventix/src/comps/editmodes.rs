// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use askama::Template;
use eventix_locale::Locale;
use std::sync::Arc;

use crate::html::filters;

#[derive(Template)]
#[template(path = "comps/editmodes.htm")]
pub struct EditModesTemplate {
    locale: Arc<dyn Locale + Send + Sync>,
    id: String,
    uid: String,
    rid: String,
}

impl EditModesTemplate {
    pub fn new<I: ToString>(
        locale: Arc<dyn Locale + Send + Sync>,
        id: I,
        uid: String,
        rid: String,
    ) -> Self {
        Self {
            locale,
            id: id.to_string(),
            uid,
            rid,
        }
    }
}
