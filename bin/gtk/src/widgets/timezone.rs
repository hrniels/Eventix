// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_locale::Locale;
use gtk::prelude::*;

/// Creates a searchable timezone dropdown initialized to the locale's timezone.
pub(crate) fn new(locale: &dyn Locale) -> gtk::DropDown {
    let zones: Vec<_> = chrono_tz::TZ_VARIANTS.iter().map(|tz| tz.name()).collect();
    let timezone = super::dropdown::from_strings(&zones);
    timezone.set_hexpand(true);
    timezone.set_factory(Some(&factory(true)));
    timezone.set_list_factory(Some(&factory(false)));
    timezone.set_expression(Some(gtk::PropertyExpression::new(
        gtk::StringObject::static_type(),
        None::<gtk::Expression>,
        "string",
    )));

    // GTK 4.12 adds substring matching; retain native prefix search on GTK 4.10.
    if timezone.find_property("search-match-mode").is_some() {
        timezone.set_property("search-match-mode", gtk::StringFilterMatchMode::Substring);
    }
    timezone.set_enable_search(true);
    connect_search(&timezone);
    if let Some(index) = zones
        .iter()
        .position(|name| *name == locale.timezone().name())
    {
        timezone.set_selected(index as u32);
    }

    timezone
}

fn connect_search(timezone: &gtk::DropDown) {
    if let (Some(search), Some(list)) = (
        descendant::<gtk::SearchEntry>(timezone.upcast_ref()),
        descendant::<gtk::ListView>(timezone.upcast_ref()),
    ) {
        if let Some(model) = list.model() {
            let weak_list = list.downgrade();
            model.connect_items_changed(move |_, _, _, _| {
                // Let GTK finish updating its selection before selecting the first match.
                let weak_list = weak_list.clone();
                gtk::glib::idle_add_local_once(move || {
                    if let Some(list) = weak_list.upgrade()
                        && let Some(model) = list.model().filter(|model| model.n_items() > 0)
                    {
                        model.select_item(0, true);
                        let _ =
                            list.activate_action("list.scroll-to-item", Some(&0u32.to_variant()));
                    }
                });
            });
        }

        // Listen inside the native popup, rather than on its dropdown outside the surface.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak_list = list.downgrade();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            if !matches!(
                key,
                gtk::gdk::Key::Down
                    | gtk::gdk::Key::KP_Down
                    | gtk::gdk::Key::Return
                    | gtk::gdk::Key::KP_Enter
                    | gtk::gdk::Key::ISO_Enter
            ) || modifiers.intersects(
                gtk::gdk::ModifierType::SHIFT_MASK
                    | gtk::gdk::ModifierType::CONTROL_MASK
                    | gtk::gdk::ModifierType::ALT_MASK
                    | gtk::gdk::ModifierType::SUPER_MASK,
            ) {
                return gtk::glib::Propagation::Proceed;
            }
            let Some(list) = weak_list.upgrade() else {
                return gtk::glib::Propagation::Proceed;
            };
            let Some(model) = list.model().filter(|model| model.n_items() > 0) else {
                return gtk::glib::Propagation::Stop;
            };
            if matches!(
                key,
                gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter | gtk::gdk::Key::ISO_Enter
            ) {
                let selected = model.selection().minimum();
                if selected < model.n_items() {
                    list.emit_by_name::<()>("activate", &[&selected]);
                }
                return gtk::glib::Propagation::Stop;
            }
            model.select_item(0, true);
            let _ = list.activate_action("list.scroll-to-item", Some(&0u32.to_variant()));
            // Scrolling must allocate the first row before it can receive focus. Grabbing
            // focus on the list itself would restore GTK's previously focused row instead.
            let weak_list = list.downgrade();
            gtk::glib::idle_add_local_once(move || {
                if let Some(list) = weak_list.upgrade().filter(|list| list.is_mapped()) {
                    focus_first_result(&list);
                }
            });
            gtk::glib::Propagation::Stop
        });
        search.add_controller(keys);
    }
}

fn focus_first_result(list: &gtk::ListView) {
    let Some(first) = list
        .model()
        .and_then(|model| model.item(0))
        .and_downcast::<gtk::StringObject>()
    else {
        return;
    };
    let mut row = list.first_child();
    while let Some(current) = row {
        if descendant::<gtk::Label>(&current).is_some_and(|label| label.text() == first.string()) {
            current.grab_focus();
            break;
        }
        row = current.next_sibling();
    }
}

fn descendant<T: IsA<gtk::Widget> + gtk::glib::object::IsClass>(widget: &gtk::Widget) -> Option<T> {
    if let Ok(child) = widget.clone().downcast::<T>() {
        return Some(child);
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(child) = descendant(&current) {
            return Some(child);
        }
        child = current.next_sibling();
    }
    None
}

fn factory(compact: bool) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };

        let label = gtk::Label::builder().xalign(0.0).build();
        if compact {
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_width_chars(1);
            label.set_max_width_chars(18);
        }
        item.set_child(Some(&label));
    });

    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };

        if let (Some(label), Some(zone)) = (
            item.child().and_downcast::<gtk::Label>(),
            item.item().and_downcast::<gtk::StringObject>(),
        ) {
            label.set_text(&zone.string());
            label.set_tooltip_text(Some(&zone.string()));
        }
    });

    factory
}

#[cfg(test)]
mod tests {
    use gtk::prelude::*;

    use super::{descendant, new};

    #[test]
    #[ignore = "requires a GTK display"]
    fn timezone_search_selects_first_match_and_enter_accepts_selection() {
        gtk::init().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        let locale = eventix_locale::new_with_timezone(
            &xdg,
            eventix_locale::LocaleType::English,
            chrono_tz::Europe::Berlin,
        )
        .unwrap();
        let timezone = new(locale.as_ref());
        let window = gtk::Window::builder().child(&timezone).build();
        window.present();
        let drain = || {
            for _ in 0..30 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        drain();
        let search = descendant::<gtk::SearchEntry>(timezone.upcast_ref()).unwrap();
        let list = descendant::<gtk::ListView>(timezone.upcast_ref()).unwrap();
        let popup = descendant::<gtk::Popover>(timezone.upcast_ref()).unwrap();
        let keys = search.observe_controllers();
        let keys = (0..keys.n_items())
            .filter_map(|index| keys.item(index).and_downcast::<gtk::EventControllerKey>())
            .find(|keys| keys.propagation_phase() == gtk::PropagationPhase::Capture)
            .unwrap();

        for (query, position) in [("Europe/", 0), ("America/", 1), ("no matching timezone", 0)] {
            timezone.activate();
            drain();
            search.set_text(query);
            drain();
            let model = list.model().unwrap();
            let expected = if model.n_items() > 0 {
                assert!(model.n_items() > 1);
                assert_eq!(model.selection().minimum(), 0);
                model.select_item(position, true);
                Some(
                    model
                        .item(position)
                        .and_downcast::<gtk::StringObject>()
                        .unwrap()
                        .string(),
                )
            } else {
                None
            };
            let previous = timezone.selected();
            assert!(keys.emit_by_name::<bool>(
                "key-pressed",
                &[
                    &gtk::gdk::Key::Return,
                    &0u32,
                    &gtk::gdk::ModifierType::empty()
                ],
            ));
            drain();
            if let Some(expected) = expected {
                assert_eq!(
                    chrono_tz::TZ_VARIANTS[timezone.selected() as usize].name(),
                    expected
                );
                assert!(!popup.is_visible());
            } else {
                assert_eq!(timezone.selected(), previous);
                assert!(popup.is_visible());
                popup.popdown();
            }
        }
        window.close();
    }
}
