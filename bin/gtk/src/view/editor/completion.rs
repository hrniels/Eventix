// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gdk, glib, prelude::*};

/// Keeps suggestions outside the form layout while typing retains focus in the entry.
pub(super) struct Completion {
    popup: gtk::Popover,
    list: gtk::ListBox,
    scroll: gtk::ScrolledWindow,
    // The parent must outlive unparenting, including during editor teardown.
    entry: gtk::Entry,
}

impl Completion {
    pub fn new(entry: &gtk::Entry, candidates: Vec<String>) -> Self {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_focusable(false);

        let (scroll, popup) = Self::create_popup(&list);
        popup.set_parent(entry);

        let completion = Self {
            popup,
            list,
            scroll,
            entry: entry.clone(),
        };

        let focus = gtk::EventControllerFocus::new();
        entry.add_controller(focus.clone());

        let dismissed = Rc::new(RefCell::new(None::<String>));
        completion.connect_refresh(&focus, candidates, dismissed.clone());
        completion.connect_activation(dismissed.clone());
        completion.connect_keys(dismissed);
        completion.connect_focus_leave(&focus);

        completion
    }

    fn create_popup(list: &gtk::ListBox) -> (gtk::ScrolledWindow, gtk::Popover) {
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .propagate_natural_height(true)
            .propagate_natural_width(true)
            .max_content_height(240)
            .child(list)
            .build();
        scroll.set_focusable(false);
        let popup = gtk::Popover::builder()
            .autohide(false)
            .has_arrow(false)
            .position(gtk::PositionType::Bottom)
            .child(&scroll)
            .build();
        popup.set_halign(gtk::Align::Start);
        (scroll, popup)
    }

    fn connect_refresh(
        &self,
        focus: &gtk::EventControllerFocus,
        candidates: Vec<String>,
        dismissed: Rc<RefCell<Option<String>>>,
    ) {
        let schedule =
            schedule_refresh(self.refresh_callback(focus, candidates, dismissed.clone()));
        let update = schedule.clone();
        self.entry.connect_changed(move |_| update());
        let update = schedule.clone();
        self.entry.connect_map(move |_| update());
        focus.connect_enter(move |_| {
            dismissed.borrow_mut().take();
            schedule();
        });
    }

    fn refresh_callback(
        &self,
        focus: &gtk::EventControllerFocus,
        candidates: Vec<String>,
        dismissed: Rc<RefCell<Option<String>>>,
    ) -> Rc<dyn Fn()> {
        let candidates: Vec<_> = candidates
            .into_iter()
            .map(|value| {
                let normalized = value.to_lowercase();
                (value, normalized)
            })
            .collect();
        let weak_popup = self.popup.downgrade();
        let weak_list = self.list.downgrade();
        let weak_entry = self.entry.downgrade();
        let weak_focus = focus.downgrade();
        let weak_scroll = self.scroll.downgrade();
        Rc::new(move || {
            let (Some(popup), Some(list), Some(entry), Some(focus)) = (
                weak_popup.upgrade(),
                weak_list.upgrade(),
                weak_entry.upgrade(),
                weak_focus.upgrade(),
            ) else {
                return;
            };
            // Reopen from a clean state: an interrupted popdown animation can otherwise hide
            // fresh results, and an already mapped native popup can retain its previous size.
            popup.set_visible(false);
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            let text = entry.text();
            if entry.is_mapped()
                && focus.contains_focus()
                && dismissed.borrow().as_deref() != Some(text.as_str())
            {
                populate_list(&list, &candidates, &text);
            }
            if list.first_child().is_some() {
                if let Some(scroll) = weak_scroll.upgrade() {
                    scroll.vadjustment().set_value(0.0);
                }
                popup.popup();
            }
        })
    }

    fn connect_activation(&self, dismissed: Rc<RefCell<Option<String>>>) {
        let weak_entry = self.entry.downgrade();
        let weak_popup = self.popup.downgrade();
        self.list.connect_row_activated(move |_, row| {
            if let (Some(entry), Some(popup)) = (weak_entry.upgrade(), weak_popup.upgrade()) {
                accept(&entry, &popup, row, &dismissed);
            }
        });
    }

    fn connect_keys(&self, dismissed: Rc<RefCell<Option<String>>>) {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak_entry = self.entry.downgrade();
        let weak_popup = self.popup.downgrade();
        let weak_list = self.list.downgrade();
        let weak_scroll = self.scroll.downgrade();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let (Some(entry), Some(popup), Some(list)) = (
                weak_entry.upgrade(),
                weak_popup.upgrade(),
                weak_list.upgrade(),
            ) else {
                return glib::Propagation::Proceed;
            };
            if !popup.is_visible()
                || modifiers
                    .intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK)
            {
                return glib::Propagation::Proceed;
            }
            match key {
                gdk::Key::Down | gdk::Key::Up => {
                    select_row(&list, weak_scroll.upgrade().as_ref(), key);
                }
                gdk::Key::Tab | gdk::Key::Return | gdk::Key::KP_Enter => {
                    if let Some(row) = list.selected_row() {
                        accept(&entry, &popup, &row, &dismissed);
                    } else {
                        dismiss(&entry, &popup, &dismissed);
                        return glib::Propagation::Proceed;
                    }
                }
                gdk::Key::Escape => {
                    dismiss(&entry, &popup, &dismissed);
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        self.entry.add_controller(keys);
    }

    fn connect_focus_leave(&self, focus: &gtk::EventControllerFocus) {
        let weak_popup = self.popup.downgrade();
        focus.connect_leave(move |_| {
            if let Some(popup) = weak_popup.upgrade() {
                popup.set_visible(false);
            }
        });
    }
}

/// Coalesces edits into a single refresh after GTK has settled the entry and focus state.
fn schedule_refresh(refresh: Rc<dyn Fn()>) -> Rc<dyn Fn()> {
    let pending = Rc::new(Cell::new(false));
    Rc::new(move || {
        if !pending.replace(true) {
            let pending = pending.clone();
            let refresh = refresh.clone();
            glib::idle_add_local_once(move || {
                pending.set(false);
                refresh();
            });
        }
    })
}

fn populate_list(list: &gtk::ListBox, candidates: &[(String, String)], query: &str) {
    for value in matches(candidates, query) {
        let label = gtk::Label::new(Some(value));
        label.set_xalign(0.0);
        label.set_margin_start(8);
        label.set_margin_end(8);
        label.set_margin_top(5);
        label.set_margin_bottom(5);
        list.append(&label);
        if let Some(row) = label.parent() {
            row.set_focusable(false);
        }
    }
}

fn select_row(list: &gtk::ListBox, scroll: Option<&gtk::ScrolledWindow>, key: gdk::Key) {
    let index = list.selected_row().map(|row| row.index());
    let next = match (key, index) {
        (gdk::Key::Down, Some(index)) => index + 1,
        (gdk::Key::Up, Some(index)) => (index - 1).max(0),
        _ => 0,
    };
    if let Some(row) = list.row_at_index(next) {
        list.select_row(Some(&row));
        if let Some(scroll) = scroll
            && let Some(bounds) = row.compute_bounds(list)
        {
            scroll
                .vadjustment()
                .clamp_page(bounds.y() as f64, (bounds.y() + bounds.height()) as f64);
        }
    }
}

fn dismiss(entry: &gtk::Entry, popup: &gtk::Popover, dismissed: &RefCell<Option<String>>) {
    *dismissed.borrow_mut() = Some(entry.text().to_string());
    popup.set_visible(false);
}

fn accept(
    entry: &gtk::Entry,
    popup: &gtk::Popover,
    row: &gtk::ListBoxRow,
    dismissed: &RefCell<Option<String>>,
) {
    if let Some(label) = row.child().and_downcast::<gtk::Label>() {
        entry.set_text(&label.text());
        entry.set_position(-1);
        *dismissed.borrow_mut() = Some(entry.text().to_string());
    }
    popup.set_visible(false);
}

/// Ranks prefix and word-boundary matches ahead of interior substrings.
fn matches<'a>(candidates: &'a [(String, String)], query: &str) -> Vec<&'a str> {
    let query = query.trim().to_lowercase();
    if query.chars().count() < 2 {
        return Vec::new();
    }
    let mut ranked: Vec<_> = candidates
        .iter()
        .filter_map(|(value, normalized)| {
            match_score(normalized, &query).map(|score| (score, value.as_str()))
        })
        .collect();
    ranked.sort_by_key(|(score, _)| *score);
    ranked
        .into_iter()
        .take(20)
        .map(|(_, value)| value)
        .collect()
}

fn match_score(normalized: &str, query: &str) -> Option<(u8, usize, usize)> {
    if normalized == query {
        return None;
    }
    let (category, position) = normalized
        .match_indices(query)
        .map(|(position, _)| {
            let category = if position == 0 {
                0
            } else if normalized[..position]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_alphanumeric())
            {
                1
            } else {
                2
            };
            (category, position)
        })
        .min()?;
    Some((category, position, normalized.len()))
}

impl Drop for Completion {
    fn drop(&mut self) {
        self.popup.unparent();
    }
}

#[cfg(test)]
mod tests {
    use super::matches;

    fn candidates(values: &[&str]) -> Vec<(String, String)> {
        values
            .iter()
            .map(|value| (value.to_string(), value.to_lowercase()))
            .collect()
    }

    #[test]
    fn ranks_prefixes_then_word_boundaries_then_interior_matches() {
        let values = candidates(&[
            "Backoffice",
            "Main Office",
            "Office annex",
            "Office",
            "Offsite",
        ]);
        assert_eq!(
            matches(&values, "OFF"),
            vec![
                "Office",
                "Offsite",
                "Office annex",
                "Main Office",
                "Backoffice"
            ]
        );
        assert_eq!(
            matches(&values, "office"),
            vec!["Office annex", "Main Office", "Backoffice"]
        );
        assert!(matches(&values, "o").is_empty());
        assert!(matches(&values, "unknown").is_empty());
    }

    #[test]
    fn matches_contact_names_and_addresses_and_handles_unicode_boundaries() {
        let values = candidates(&[
            "Joann <joann@example.org>",
            "Ann <ann@example.org>",
            "Équipe — Ann",
            "Bob <ann@example.org>",
        ]);
        assert_eq!(
            matches(&values, " ann "),
            vec![
                "Ann <ann@example.org>",
                "Bob <ann@example.org>",
                "Équipe — Ann",
                "Joann <joann@example.org>"
            ]
        );
        assert_eq!(matches(&values, "ÉQUIPE"), vec!["Équipe — Ann"]);
        assert_eq!(matches(&values, "example.org").len(), 3);
    }

    #[test]
    fn limits_results_after_ranking() {
        let mut values: Vec<_> = (0..30)
            .map(|index| {
                let value = format!("Room {index:02}");
                (value.clone(), value.to_lowercase())
            })
            .collect();
        values.push(("Ro".into(), "ro".into()));
        values.push(("Roof".into(), "roof".into()));
        let results = matches(&values, "ro");
        assert_eq!(results.len(), 20);
        assert_eq!(results[0], "Roof");
    }
}
