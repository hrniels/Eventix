// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use askama::Template;
use chrono::NaiveTime;
use serde::{Deserialize, Serialize};

use crate::html::filters;

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(default, transparent)]
pub struct Time {
    value: NaiveTime,
}

impl Time {
    pub fn new(value: NaiveTime) -> Self {
        Self { value }
    }

    pub fn value(&self) -> NaiveTime {
        self.value
    }
}

#[derive(Template)]
#[template(path = "comps/time.htm")]
pub struct TimeTemplate {
    name: String,
    id: String,
    value: Option<NaiveTime>,
}

impl TimeTemplate {
    #[allow(dead_code)]
    pub fn new<N: ToString>(name: N, time: Option<Time>) -> Self {
        let name = name.to_string();
        Self {
            id: name.replace("[", "_").replace("]", "_"),
            name,
            value: time.map(|t| t.value()),
        }
    }
}
