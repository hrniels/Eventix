// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_ical::objects::EventLike;

/// Case-insensitive AND groups separated by the standalone word `OR`.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct KeywordExpression(Vec<Vec<String>>);

impl KeywordExpression {
    pub fn parse(input: &str) -> Self {
        let mut groups = Vec::new();
        let mut current = Vec::new();
        for token in input.split_whitespace() {
            if token.eq_ignore_ascii_case("OR") {
                if !current.is_empty() {
                    groups.push(std::mem::take(&mut current));
                }
            } else {
                current.push(token.to_lowercase());
            }
        }
        if !current.is_empty() {
            groups.push(current);
        }
        Self(groups)
    }

    pub fn matches(&self, text: &str) -> bool {
        let text = text.to_lowercase();
        self.0.is_empty()
            || self
                .0
                .iter()
                .any(|group| group.iter().all(|term| text.contains(term)))
    }

    pub fn matches_event(&self, event: &impl EventLike) -> bool {
        self.matches(
            &[
                event.summary().map(String::as_str),
                event.description().map(String::as_str),
                event.location().map(String::as_str),
                Some(event.uid().as_str()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" "),
        )
    }
}
