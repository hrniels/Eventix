// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_locale::Locale;
use gtk::prelude::*;

/// Grouped toggle buttons backed by a single selection model.
pub(crate) struct SegmentedControl {
    pub widget: gtk::Box,
    pub selection: gtk::SingleSelection,
    buttons: Vec<gtk::ToggleButton>,
}

impl SegmentedControl {
    pub fn new(locale: &dyn Locale, labels: &[&str], selected: u32) -> Self {
        let names: Vec<_> = labels.iter().map(|label| locale.translate(label)).collect();
        let selection = gtk::SingleSelection::new(Some(gtk::StringList::new(&names)));
        selection.set_selected(selected);
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        widget.add_css_class("linked");
        let mut buttons = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let button = gtk::ToggleButton::with_label(name);
            button.set_group(buttons.first());
            button.set_active(selection.selected() == index as u32);
            let model = selection.clone();
            button.connect_toggled(move |button| {
                if button.is_active() {
                    model.set_selected(index as u32);
                }
            });
            let weak = button.downgrade();
            selection.connect_selected_notify(move |selection| {
                if let Some(button) = weak.upgrade() {
                    button.set_active(selection.selected() == index as u32);
                }
            });
            widget.append(&button);
            buttons.push(button);
        }
        Self {
            widget,
            selection,
            buttons,
        }
    }

    pub fn set_option_sensitive(&self, index: usize, sensitive: bool) {
        self.buttons[index].set_sensitive(sensitive);
    }
}

#[cfg(test)]
mod tests {
    use gtk::prelude::*;

    use super::SegmentedControl;

    #[test]
    #[ignore = "requires a GTK display"]
    fn selection_and_buttons_stay_in_sync() {
        gtk::init().unwrap();
        let control = SegmentedControl::new(
            eventix_locale::default().as_ref(),
            &["Calendar", "Personal"],
            1,
        );
        assert!(!control.buttons[0].is_active());
        assert!(control.buttons[1].is_active());

        control.buttons[0].set_active(true);
        assert_eq!(control.selection.selected(), 0);
        assert!(!control.buttons[1].is_active());
        control.selection.set_selected(1);
        assert!(control.buttons[1].is_active());
        assert!(!control.buttons[0].is_active());

        control.set_option_sensitive(1, false);
        assert!(!control.buttons[1].is_sensitive());
    }
}
