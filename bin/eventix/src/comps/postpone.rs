// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use askama::Template;
use eventix_locale::Locale;
use std::sync::Arc;

use crate::html::filters;

#[derive(Template)]
#[template(path = "comps/postpone.htm")]
pub struct PostponeTemplate {
    locale: Arc<dyn Locale + Send + Sync>,
    id: String,
    uid: String,
    rid: Option<String>,
}

impl PostponeTemplate {
    pub fn new<I: ToString>(
        locale: Arc<dyn Locale + Send + Sync>,
        id: I,
        uid: String,
        rid: Option<String>,
    ) -> Self {
        Self {
            locale,
            id: id.to_string(),
            uid,
            rid,
        }
    }
}
