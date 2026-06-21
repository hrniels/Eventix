// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use askama::Template;
use eventix_locale::Locale;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PasswordRequest {
    password: String,
}

impl PasswordRequest {
    pub fn get(&self) -> &String {
        &self.password
    }

    pub fn check(
        &self,
        locale: &Arc<dyn Locale + Send + Sync>,
        page: &mut crate::pages::Page,
        is_add: bool,
    ) -> bool {
        if is_add && self.password.is_empty() {
            page.add_error(locale.translate("error.collection_password").to_string());
            false
        } else {
            true
        }
    }
}

#[derive(Template)]
#[template(path = "comps/password.htm")]
pub struct PasswordTemplate {
    name: String,
    id: String,
}

impl PasswordTemplate {
    pub fn new(name: String) -> Self {
        Self {
            id: name.replace("[", "_").replace("]", "_"),
            name,
        }
    }
}
