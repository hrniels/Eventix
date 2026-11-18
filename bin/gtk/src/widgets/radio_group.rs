// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::{gdk, prelude::*};

/// Enables wrapping arrow-key selection in display order, skipping unavailable choices.
pub(crate) fn enable_arrow_keys(buttons: &[gtk::CheckButton]) {
    let group: Vec<_> = buttons.iter().map(ObjectExt::downgrade).collect();
    for (index, button) in buttons.iter().enumerate() {
        let group = group.clone();
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
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
                gdk::Key::Down | gdk::Key::Right | gdk::Key::KP_Down | gdk::Key::KP_Right => true,
                gdk::Key::Up | gdk::Key::Left | gdk::Key::KP_Up | gdk::Key::KP_Left => false,
                _ => return gtk::glib::Propagation::Proceed,
            };
            for offset in 1..=group.len() {
                let next = if forward {
                    (index + offset) % group.len()
                } else {
                    (index + group.len() - offset) % group.len()
                };
                if let Some(button) = group[next].upgrade()
                    && button.is_mapped()
                    && button.is_sensitive()
                    && button.grab_focus()
                {
                    button.set_active(true);
                    break;
                }
            }
            gtk::glib::Propagation::Stop
        });
        button.add_controller(keys);
    }
}

#[cfg(test)]
mod tests {
    use gtk::{gdk, prelude::*};

    use super::enable_arrow_keys;

    #[test]
    #[ignore = "requires a GTK display"]
    fn radio_arrows_select_focus_wrap_and_skip_unavailable_choices() {
        gtk::init().unwrap();
        let window = gtk::Window::new();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let buttons: Vec<_> = (0..4).map(|_| gtk::CheckButton::new()).collect();
        let mut rows = Vec::new();
        for button in &buttons {
            button.set_group(Some(&buttons[0]));
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            row.append(button);
            row.append(&gtk::Entry::new());
            content.append(&row);
            rows.push(row);
        }
        enable_arrow_keys(&buttons);
        window.set_child(Some(&content));
        window.present();
        while gtk::glib::MainContext::default().iteration(false) {}

        let press = |index: usize, key, modifiers| {
            let controllers = buttons[index].observe_controllers();
            let keys = (0..controllers.n_items())
                .filter_map(|i| {
                    controllers
                        .item(i)
                        .and_downcast::<gtk::EventControllerKey>()
                })
                .find(|keys| keys.propagation_phase() == gtk::PropagationPhase::Capture)
                .unwrap();
            keys.emit_by_name::<bool>("key-pressed", &[&key, &0u32, &modifiers])
        };
        buttons[0].set_active(true);
        buttons[0].grab_focus();
        for (from, key, to) in [
            (0, gdk::Key::Down, 1),
            (1, gdk::Key::Right, 2),
            (2, gdk::Key::Up, 1),
            (1, gdk::Key::Left, 0),
            (0, gdk::Key::KP_Up, 3),
            (3, gdk::Key::KP_Right, 0),
            (0, gdk::Key::KP_Down, 1),
            (1, gdk::Key::KP_Left, 0),
        ] {
            assert!(press(from, key, gdk::ModifierType::empty()));
            assert!(buttons[to].is_active());
            assert!(buttons[to].has_focus());
            assert_eq!(
                buttons.iter().filter(|button| button.is_active()).count(),
                1
            );
        }

        rows[1].set_visible(false);
        rows[2].set_sensitive(false);
        assert!(press(0, gdk::Key::Down, gdk::ModifierType::empty()));
        assert!(buttons[3].is_active() && buttons[3].has_focus());
        for modifiers in [
            gdk::ModifierType::SHIFT_MASK,
            gdk::ModifierType::CONTROL_MASK,
            gdk::ModifierType::ALT_MASK,
            gdk::ModifierType::SUPER_MASK,
        ] {
            assert!(!press(3, gdk::Key::Down, modifiers));
        }
        for key in [gdk::Key::Tab, gdk::Key::space, gdk::Key::Return] {
            assert!(!press(3, key, gdk::ModifierType::empty()));
        }
        buttons[0].set_sensitive(false);
        assert!(press(3, gdk::Key::Down, gdk::ModifierType::empty()));
        assert!(buttons[3].is_active() && buttons[3].has_focus());
        window.close();
    }
}
