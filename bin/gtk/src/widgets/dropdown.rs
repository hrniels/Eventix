// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::{gdk, prelude::*};

/// Creates a native dropdown with collapsed arrow-key and mouse-wheel selection.
pub(crate) fn from_strings(strings: &[&str]) -> gtk::DropDown {
    let widget = gtk::DropDown::from_strings(strings);
    enable_navigation(&widget);
    widget
}

/// Creates a native dropdown sharing a model, with collapsed arrow-key and wheel selection.
pub(crate) fn from_model(model: Option<&gtk::gio::ListModel>) -> gtk::DropDown {
    let widget = gtk::DropDown::new(model.cloned(), None::<gtk::Expression>);
    enable_navigation(&widget);
    widget
}

/// Enables collapsed selection while leaving the popup's navigation to GTK.
///
/// Call once for dropdowns loaded from a UI resource. Unmodified vertical arrows select
/// adjacent items without wrapping, as does the mouse wheel without requiring focus.
/// Horizontal arrows keep focus and selection unchanged.
pub(crate) fn enable_navigation(widget: &gtk::DropDown) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = widget.downgrade();
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        if modifiers.intersects(
            gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::SUPER_MASK
                | gdk::ModifierType::SHIFT_MASK,
        ) {
            return gtk::glib::Propagation::Proceed;
        }
        let Some(widget) = weak.upgrade() else {
            return gtk::glib::Propagation::Proceed;
        };
        let mut focused = widget.root().and_then(|root| root.focus());
        let mut owns_focus = false;
        while let Some(child) = focused {
            if child.is::<gtk::Popover>() {
                return gtk::glib::Propagation::Proceed;
            }
            if child == widget {
                owns_focus = true;
                break;
            }
            focused = child.parent();
        }
        if !owns_focus {
            return gtk::glib::Propagation::Proceed;
        }

        let forward = match key {
            gdk::Key::Down | gdk::Key::KP_Down => true,
            gdk::Key::Up | gdk::Key::KP_Up => false,
            gdk::Key::Left | gdk::Key::Right | gdk::Key::KP_Left | gdk::Key::KP_Right => {
                return gtk::glib::Propagation::Stop;
            }
            _ => return gtk::glib::Propagation::Proceed,
        };
        select_adjacent(&widget, forward);
        gtk::glib::Propagation::Stop
    });
    widget.add_controller(keys);

    let scroll = gtk::EventControllerScroll::new(
        gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE,
    );
    scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = widget.downgrade();
    scroll.connect_scroll(move |_, _, dy| {
        let Some(widget) = weak.upgrade() else {
            return gtk::glib::Propagation::Proceed;
        };
        if dy == 0.0 || has_open_popup(widget.upcast_ref()) {
            return gtk::glib::Propagation::Proceed;
        }
        select_adjacent(&widget, dy > 0.0);
        gtk::glib::Propagation::Stop
    });
    widget.add_controller(scroll);
}

fn select_adjacent(widget: &gtk::DropDown, forward: bool) {
    if let Some(model) = widget.model() {
        let selected = widget.selected();
        let next = if selected == gtk::INVALID_LIST_POSITION {
            Some(if forward {
                0
            } else {
                model.n_items().saturating_sub(1)
            })
        } else if forward {
            selected.checked_add(1)
        } else {
            selected.checked_sub(1)
        };
        if let Some(next) = next.filter(|&next| next < model.n_items()) {
            widget.set_selected(next);
        }
    }
}

fn has_open_popup(widget: &gtk::Widget) -> bool {
    if widget.is::<gtk::Popover>() {
        return widget.is_visible();
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        if has_open_popup(&widget) {
            return true;
        }
        child = widget.next_sibling();
    }
    false
}

#[cfg(test)]
mod tests {
    use gtk::{gdk, prelude::*};

    use super::{from_model, from_strings};

    fn drain_events() {
        // Native button activation is delayed until its activation animation completes.
        for _ in 0..30 {
            while gtk::glib::MainContext::default().iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn controller(widget: &gtk::DropDown) -> gtk::EventControllerKey {
        let controllers = widget.observe_controllers();
        (0..controllers.n_items())
            .filter_map(|index| {
                controllers
                    .item(index)
                    .and_downcast::<gtk::EventControllerKey>()
            })
            .find(|keys| keys.propagation_phase() == gtk::PropagationPhase::Capture)
            .unwrap()
    }

    fn press(keys: &gtk::EventControllerKey, key: gdk::Key) -> bool {
        keys.emit_by_name("key-pressed", &[&key, &0u32, &gdk::ModifierType::empty()])
    }

    fn scroll_controller(widget: &gtk::DropDown) -> gtk::EventControllerScroll {
        let controllers = widget.observe_controllers();
        (0..controllers.n_items())
            .filter_map(|index| {
                controllers
                    .item(index)
                    .and_downcast::<gtk::EventControllerScroll>()
            })
            .find(|scroll| scroll.propagation_phase() == gtk::PropagationPhase::Capture)
            .unwrap()
    }

    fn child<T: IsA<gtk::Widget> + StaticType>(widget: &gtk::Widget) -> Option<T> {
        if let Ok(found) = widget.clone().downcast::<T>() {
            return Some(found);
        }
        let mut next = widget.first_child();
        while let Some(widget) = next {
            if let Some(found) = child::<T>(&widget) {
                return Some(found);
            }
            next = widget.next_sibling();
        }
        None
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn collapsed_arrows_select_without_wrapping_opening_or_moving_focus() {
        gtk::init().unwrap();
        let widget = from_strings(&["First", "Second", "Third"]);
        let window = gtk::Window::builder().child(&widget).build();
        window.present();
        drain_events();
        assert!(widget.grab_focus());
        let focus = GtkWindowExt::focus(&window);
        let keys = controller(&widget);
        for (key, selected) in [
            (gdk::Key::Up, 0),
            (gdk::Key::Down, 1),
            (gdk::Key::KP_Down, 2),
            (gdk::Key::Down, 2),
            (gdk::Key::KP_Up, 1),
            (gdk::Key::Up, 0),
        ] {
            assert!(press(&keys, key));
            assert_eq!(widget.selected(), selected);
            assert_eq!(GtkWindowExt::focus(&window), focus);
            assert!(
                !child::<gtk::Popover>(widget.upcast_ref())
                    .unwrap()
                    .is_visible()
            );
        }
        for key in [
            gdk::Key::Left,
            gdk::Key::Right,
            gdk::Key::KP_Left,
            gdk::Key::KP_Right,
        ] {
            assert!(press(&keys, key));
            assert_eq!(widget.selected(), 0);
        }
        for modifiers in [
            gdk::ModifierType::SHIFT_MASK,
            gdk::ModifierType::CONTROL_MASK,
            gdk::ModifierType::ALT_MASK,
            gdk::ModifierType::SUPER_MASK,
        ] {
            assert!(
                !keys.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Down, &0u32, &modifiers],)
            );
            assert_eq!(widget.selected(), 0);
        }
        for key in [
            gdk::Key::space,
            gdk::Key::Return,
            gdk::Key::Escape,
            gdk::Key::Tab,
        ] {
            assert!(!press(&keys, key));
        }
        window.close();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn collapsed_wheel_selects_without_wrapping_opening_or_taking_focus() {
        gtk::init().unwrap();
        let widget = from_strings(&["First", "Second", "Third"]);
        let entry = gtk::Entry::new();
        let layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
        layout.append(&entry);
        layout.append(&widget);
        let window = gtk::Window::builder().child(&layout).build();
        window.present();
        drain_events();
        assert!(entry.grab_focus());
        let focus = GtkWindowExt::focus(&window);
        let scroll = scroll_controller(&widget);
        assert_eq!(
            scroll.flags(),
            gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE
        );
        for (dy, selected) in [
            (-1.0, 0),
            (1.0, 1),
            (1.0, 2),
            (1.0, 2),
            (-1.0, 1),
            (-1.0, 0),
        ] {
            assert!(scroll.emit_by_name::<bool>("scroll", &[&0.0f64, &dy]));
            assert_eq!(widget.selected(), selected);
            assert_eq!(GtkWindowExt::focus(&window), focus);
            assert!(!super::has_open_popup(widget.upcast_ref()));
        }
        assert!(!scroll.emit_by_name::<bool>("scroll", &[&1.0f64, &0.0f64]));
        assert_eq!(widget.selected(), 0);
        window.close();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn model_changes_empty_models_and_missing_selections_are_safe() {
        gtk::init().unwrap();
        let model = gtk::StringList::new(&["First", "Second"]).upcast::<gtk::gio::ListModel>();
        let widget = from_model(Some(&model));
        let window = gtk::Window::builder().child(&widget).build();
        window.present();
        drain_events();
        assert!(widget.grab_focus());
        let keys = controller(&widget);
        let scroll = scroll_controller(&widget);
        for (key, from_invalid, from_first, from_last) in
            [(gdk::Key::Down, 0, 1, 1), (gdk::Key::Up, 1, 0, 0)]
        {
            // GTK may accept the invalid selection, autoselect, or keep the current item.
            widget.set_selected(gtk::INVALID_LIST_POSITION);
            let expected = match widget.selected() {
                gtk::INVALID_LIST_POSITION => from_invalid,
                0 => from_first,
                1 => from_last,
                selected => panic!("unexpected selection in two-item model: {selected}"),
            };
            assert!(press(&keys, key));
            assert_eq!(widget.selected(), expected);
        }
        for strings in [&["Only"][..], &[][..]] {
            widget.set_model(Some(&gtk::StringList::new(strings)));
            let selected = widget.selected();
            assert!(press(&keys, gdk::Key::Down));
            assert!(press(&keys, gdk::Key::Up));
            assert!(scroll.emit_by_name::<bool>("scroll", &[&0.0f64, &1.0f64]));
            assert!(scroll.emit_by_name::<bool>("scroll", &[&0.0f64, &-1.0f64]));
            assert_eq!(widget.selected(), selected);
        }
        widget.set_model(None::<&gtk::gio::ListModel>);
        assert!(press(&keys, gdk::Key::Down));
        assert!(press(&keys, gdk::Key::Up));
        assert!(scroll.emit_by_name::<bool>("scroll", &[&0.0f64, &1.0f64]));
        assert!(scroll.emit_by_name::<bool>("scroll", &[&0.0f64, &-1.0f64]));
        assert_eq!(widget.selected(), gtk::INVALID_LIST_POSITION);
        window.close();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn builder_dropdown_keeps_native_popup_search_and_list_navigation() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let now = chrono::Utc::now().naive_utc();
        let input = crate::widgets::AlarmInput::new(
            eventix_locale::default().as_ref(),
            now.date(),
            now.time(),
        );
        input.trigger.set_selected(1);
        let widget = input.unit;
        widget.set_model(Some(&gtk::StringList::new(&["First", "Second", "Third"])));
        widget.set_expression(Some(gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            None::<gtk::Expression>,
            "string",
        )));
        widget.set_enable_search(true);
        let window = gtk::Window::builder().child(&input.widget).build();
        window.present();
        drain_events();
        assert!(widget.grab_focus());
        let keys = controller(&widget);
        assert!(press(&keys, gdk::Key::Down));
        assert_eq!(widget.selected(), 1);
        assert!(widget.activate());
        drain_events();
        let popup = child::<gtk::Popover>(widget.upcast_ref()).unwrap();
        assert!(popup.is_visible());
        let scroll = scroll_controller(&widget);
        for dy in [-1.0f64, 1.0f64] {
            assert!(!scroll.emit_by_name::<bool>("scroll", &[&0.0f64, &dy]));
            assert_eq!(widget.selected(), 1);
        }
        let search = child::<gtk::SearchEntry>(popup.upcast_ref()).unwrap();
        assert!(search.grab_focus());
        for key in [
            gdk::Key::Up,
            gdk::Key::Down,
            gdk::Key::Left,
            gdk::Key::Right,
        ] {
            assert!(!press(&keys, key));
            assert_eq!(widget.selected(), 1);
        }
        let list = child::<gtk::ListView>(popup.upcast_ref()).unwrap();
        search.set_text("Third");
        drain_events();
        assert_eq!(list.model().unwrap().n_items(), 1);
        search.set_text("");
        drain_events();
        assert_eq!(list.model().unwrap().n_items(), 3);
        assert!(list.grab_focus());
        assert!(!press(&keys, gdk::Key::Down));
        assert!(!press(&keys, gdk::Key::Up));
        list.model().unwrap().select_item(2, true);
        list.emit_by_name::<()>("activate", &[&2u32]);
        drain_events();
        assert_eq!(widget.selected(), 2);
        assert!(!popup.is_visible());
        assert!(widget.grab_focus());
        assert!(press(&keys, gdk::Key::Up));
        assert_eq!(widget.selected(), 1);
        window.close();
    }
}
