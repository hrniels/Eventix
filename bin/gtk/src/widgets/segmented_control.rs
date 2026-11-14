// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_locale::Locale;
use gtk::{gdk, prelude::*};

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
        setup_keyboard_navigation(&widget, &selection, &buttons);
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

/// Keeps only the selected available segment in the Tab order.
fn update_tab_stop(buttons: &[gtk::glib::WeakRef<gtk::ToggleButton>], selected: u32) {
    let buttons: Vec<_> = buttons
        .iter()
        .filter_map(|button| button.upgrade())
        .collect();
    let available = |button: &&gtk::ToggleButton| button.is_visible() && button.is_sensitive();
    let selected = buttons
        .get(selected as usize)
        .filter(available)
        .or_else(|| buttons.iter().find(available));
    for button in &buttons {
        button.set_focusable(Some(button) == selected);
    }
}

fn setup_keyboard_navigation(
    widget: &gtk::Box,
    selection: &gtk::SingleSelection,
    buttons: &[gtk::ToggleButton],
) {
    let group: Vec<_> = buttons.iter().map(ObjectExt::downgrade).collect();
    update_tab_stop(&group, selection.selected());
    let weak_group = group.clone();
    selection.connect_selected_notify(move |selection| {
        update_tab_stop(&weak_group, selection.selected());
    });
    for button in buttons {
        for property in ["sensitive", "visible"] {
            let group = group.clone();
            let selection = selection.downgrade();
            button.connect_notify_local(Some(property), move |_, _| {
                if let Some(selection) = selection.upgrade() {
                    update_tab_stop(&group, selection.selected());
                }
            });
        }
    }

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let selection = selection.downgrade();
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        if modifiers.intersects(
            gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::SUPER_MASK
                | gdk::ModifierType::SHIFT_MASK,
        ) {
            return gtk::glib::Propagation::Proceed;
        }
        let forward = match key {
            gdk::Key::Right | gdk::Key::KP_Right => true,
            gdk::Key::Left | gdk::Key::KP_Left => false,
            _ => return gtk::glib::Propagation::Proceed,
        };
        let Some(index) = group
            .iter()
            .position(|button| button.upgrade().is_some_and(|button| button.has_focus()))
        else {
            return gtk::glib::Propagation::Proceed;
        };
        if let Some(selection) = selection.upgrade() {
            for offset in 1..=group.len() {
                let next = if forward {
                    (index + offset) % group.len()
                } else {
                    (index + group.len() - offset) % group.len()
                };
                if let Some(button) = group[next].upgrade()
                    && button.is_mapped()
                    && button.is_sensitive()
                {
                    selection.set_selected(next as u32);
                    button.grab_focus();
                    break;
                }
            }
        }
        gtk::glib::Propagation::Stop
    });
    widget.add_controller(keys);
}

#[cfg(test)]
mod tests {
    use gtk::{gdk, prelude::*};

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

    #[test]
    #[ignore = "requires a GTK display"]
    fn segmented_control_is_one_tab_stop_with_horizontal_arrow_selection() {
        gtk::init().unwrap();
        let control = SegmentedControl::new(
            eventix_locale::default().as_ref(),
            &["Calendar", "Personal", "None"],
            1,
        );
        let window = gtk::Window::new();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let before = gtk::Button::with_label("Before");
        let after = gtk::Button::with_label("After");
        content.append(&before);
        content.append(&control.widget);
        content.append(&after);
        window.set_child(Some(&content));
        window.present();
        while gtk::glib::MainContext::default().iteration(false) {}

        before.grab_focus();
        assert!(window.child_focus(gtk::DirectionType::TabForward));
        assert!(control.buttons[1].has_focus());
        assert!(window.child_focus(gtk::DirectionType::TabForward));
        assert!(after.has_focus());
        assert!(window.child_focus(gtk::DirectionType::TabBackward));
        assert!(control.buttons[1].has_focus());
        assert!(window.child_focus(gtk::DirectionType::TabBackward));
        assert!(before.has_focus());

        let controllers = control.widget.observe_controllers();
        let keys = (0..controllers.n_items())
            .filter_map(|index| {
                controllers
                    .item(index)
                    .and_downcast::<gtk::EventControllerKey>()
            })
            .find(|keys| keys.propagation_phase() == gtk::PropagationPhase::Capture)
            .unwrap();
        let press =
            |key, modifiers| keys.emit_by_name::<bool>("key-pressed", &[&key, &0u32, &modifiers]);
        control.buttons[1].grab_focus();
        for (key, expected) in [
            (gdk::Key::Right, 2),
            (gdk::Key::KP_Right, 0),
            (gdk::Key::Left, 2),
            (gdk::Key::KP_Left, 1),
        ] {
            assert!(press(key, gdk::ModifierType::empty()));
            assert_eq!(control.selection.selected(), expected as u32);
            assert!(control.buttons[expected].has_focus());
            assert!(control.buttons[expected].is_active());
        }
        control.set_option_sensitive(2, false);
        assert!(press(gdk::Key::Right, gdk::ModifierType::empty()));
        assert!(control.buttons[0].has_focus());
        control.buttons[1].set_visible(false);
        assert!(press(gdk::Key::Right, gdk::ModifierType::empty()));
        assert!(control.buttons[0].has_focus());
        assert!(!press(gdk::Key::Left, gdk::ModifierType::CONTROL_MASK));
        assert!(!press(gdk::Key::Tab, gdk::ModifierType::empty()));
        assert!(window.child_focus(gtk::DirectionType::TabForward));
        assert!(after.has_focus());

        control.buttons[1].set_visible(true);
        control.selection.set_selected(1);
        assert!(window.child_focus(gtk::DirectionType::TabBackward));
        assert!(control.buttons[1].has_focus());
        control.set_option_sensitive(1, false);
        after.grab_focus();
        assert!(window.child_focus(gtk::DirectionType::TabBackward));
        assert!(control.buttons[0].has_focus());
        control.set_option_sensitive(1, true);
        before.grab_focus();
        assert!(window.child_focus(gtk::DirectionType::TabForward));
        assert!(control.buttons[1].has_focus());
        window.close();
    }
}
