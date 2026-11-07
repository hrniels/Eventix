// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::LazyLock;

use regex::Regex;

pub(super) struct TextLink<'a> {
    pub start: usize,
    pub end: usize,
    pub text: &'a str,
    pub uri: String,
}

/// Recognizes calendar-text links, preserving the label's URI normalization and punctuation.
pub(super) fn links(text: &str) -> impl Iterator<Item = TextLink<'_>> {
    static LINKS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?:https?://|mailto:)?(?:www\.)?[-a-zA-Z0-9@:%._+~#=]{1,256}",
            r"\.[a-zA-Z0-9]{2,}\b(?:[-a-zA-Z0-9()@:;%_+.~#?&/=]*)"
        ))
        .expect("valid link regex")
    });
    LINKS.find_iter(text).map(|found| {
        let display = found.as_str().trim_end_matches(['.', ',', ';', ')']);
        let uri = if display.starts_with("https:")
            || display.starts_with("http:")
            || display.starts_with("mailto:")
        {
            display.to_owned()
        } else if display.contains('@') {
            format!("mailto:{display}")
        } else {
            format!("https://{display}")
        };
        TextLink {
            start: found.start(),
            end: found.start() + display.len(),
            text: display,
            uri,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::links;

    #[test]
    fn links_preserve_uri_targets_and_exclude_trailing_punctuation() {
        let text = "Grüße 😀 https://example.org/?a=1&b=2), http://example.net; \
                    mailto:amy@example.org, amy@example.org. www.example.org example.com";
        let found: Vec<_> = links(text).collect();
        let targets: Vec<_> = found.iter().map(|link| link.uri.as_str()).collect();
        assert_eq!(
            targets,
            [
                "https://example.org/?a=1&b=2",
                "http://example.net",
                "mailto:amy@example.org",
                "mailto:amy@example.org",
                "https://www.example.org",
                "https://example.com"
            ]
        );
        for link in found {
            assert_eq!(&text[link.start..link.end], link.text);
        }
        assert!(links("Literal <markup> & plain text").next().is_none());
    }
}
