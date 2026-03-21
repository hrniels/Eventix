// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use askama::Template;
use chrono_tz::TZ_VARIANTS;

/// All IANA timezone names, built once at compile time from `chrono_tz`.
pub fn tz_names() -> Vec<&'static str> {
    TZ_VARIANTS.iter().map(|tz| tz.name()).collect()
}

#[derive(Template)]
#[template(path = "comps/tzcombo.htm")]
pub struct TzComboTemplate {
    name: String,
    id: String,
    selected: String,
}

impl TzComboTemplate {
    pub fn new<N: ToString>(name: N, selected: String) -> Self {
        let name = name.to_string();
        Self {
            id: name.replace(['[', ']'], "_"),
            name,
            selected,
        }
    }
}
