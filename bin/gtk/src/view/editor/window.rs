// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::rc::Rc;

use gtk::{glib, prelude::*};

use super::{ItemEditor, has_nested_popup, is_save_key};

impl ItemEditor {
    /// Creates a non-modal editor window whose shortcuts do not affect the main window.
    pub(crate) fn create_window(self: &Rc<Self>, parent: &gtk::Window) -> gtk::Window {
        let panel = self.content();
        self.panel.set_halign(gtk::Align::Fill);
        self.panel.set_valign(gtk::Align::Fill);
        self.panel.set_vexpand(true);
        self.scroll.set_vexpand(true);
        self.scroll.set_propagate_natural_height(true);
        let window = gtk::Window::builder()
            .title(&self.title)
            .transient_for(parent)
            .destroy_with_parent(true)
            .modal(false)
            .resizable(false)
            .default_width(super::super::ITEM_PANEL_WIDTH)
            .css_classes(["ev_editor_window"])
            .child(&panel)
            .build();
        window.set_application(parent.application().as_ref());
        self.fit_window(&window);

        // Follow the revealers' natural height throughout expansion and collapse, rather than
        // retaining the largest size GTK has allocated. Native popups are outside this layout.
        let editor = Rc::downgrade(self);
        let previous = std::cell::Cell::new(window.default_size());
        window.add_tick_callback(move |window, _| {
            let Some(editor) = editor.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let size = editor.window_size(window);
            if previous.replace(size) != size {
                window.set_default_size(size.0, size.1);
            }
            glib::ControlFlow::Continue
        });

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let editor = Rc::downgrade(self);
        let weak = window.downgrade();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let (Some(window), Some(editor)) = (weak.upgrade(), editor.upgrade()) else {
                return glib::Propagation::Proceed;
            };
            if crate::window::handle_input_key(&window, key, modifiers) == glib::Propagation::Stop {
                return glib::Propagation::Stop;
            }
            if is_save_key(key, modifiers) {
                editor.save.emit_clicked();
                return glib::Propagation::Stop;
            }
            if key == gtk::gdk::Key::Escape && !has_nested_popup(&window) {
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(keys);

        window
    }

    fn fit_window(&self, window: &gtk::Window) {
        let (width, height) = self.window_size(window);
        window.set_default_size(width, height);
    }

    fn window_size(&self, window: &gtk::Window) -> (i32, i32) {
        let width = window.width().max(super::super::ITEM_PANEL_WIDTH);
        let surface = window
            .surface()
            .or_else(|| window.transient_for()?.surface());
        let screen_height = surface
            .and_then(|surface| surface.display().monitor_at_surface(&surface))
            .map_or(800, |monitor| monitor.geometry().height());
        let chrome = self.panel.measure(gtk::Orientation::Vertical, width).0
            - self.scroll.measure(gtk::Orientation::Vertical, width).0;
        // Leave room for decorations and desktop panels; excess form content remains scrollable.
        self.scroll
            .set_max_content_height((screen_height - chrome - 96).max(1));
        let height = self.panel.measure(gtk::Orientation::Vertical, width).1;
        (width, height)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use gtk::{gdk, glib, prelude::*};

    use super::ItemEditor;
    use crate::model::editor::{EditorCalendar, EditorOptions};

    fn drain() {
        for _ in 0..35 {
            while glib::MainContext::default().iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn editor_window_routes_nested_picker_keys_and_scrolls_expanded_sections() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gdk::Display::default().unwrap());
        let editor = ItemEditor::new(
            1,
            EditorOptions {
                kind: eventix_ical::objects::CalCompType::Event,
                calendars: vec![EditorCalendar {
                    id: "test".into(),
                    name: "Test".into(),
                    background: "#27c281".into(),
                    personal: false,
                }],
                last_calendar: None,
                locations: vec!["Office".into()],
                contacts: vec![],
                start: "2026-10-15T10:00:00".parse().unwrap(),
                locale: eventix_locale::default(),
            },
        );
        let parent = gtk::Window::new();
        parent.present();
        let window = editor.create_window(&parent);
        assert!(!window.is_resizable());
        window.present();
        assert_eq!(
            editor.panel.first_child(),
            Some(editor.scroll.clone().upcast())
        );
        let saves = Rc::new(Cell::new(0));
        let count = saves.clone();
        editor
            .save
            .connect_clicked(move |_| count.set(count.get() + 1));
        let keys = window
            .observe_controllers()
            .snapshot()
            .into_iter()
            .find_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
            .unwrap();
        let press =
            |key, modifiers| keys.emit_by_name::<bool>("key-pressed", &[&key, &0u32, &modifiers]);
        editor.focus();
        drain();
        let collapsed_height = window.height();
        assert_eq!(
            collapsed_height,
            editor
                .panel
                .measure(gtk::Orientation::Vertical, window.width())
                .1
        );
        assert!(press(gdk::Key::Escape, gdk::ModifierType::empty()));
        assert!(window.is_visible());
        assert_eq!(saves.get(), 0);
        for key in [gdk::Key::Return, gdk::Key::KP_Enter] {
            assert!(press(key, gdk::ModifierType::CONTROL_MASK));
        }
        assert_eq!(saves.get(), 2);

        editor.sections[2].toggle.set_active(true);
        drain();
        let description_scroll = editor
            .description
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>()
            .unwrap();
        let handle = description_scroll.next_sibling().unwrap();
        let content = description_scroll.parent().unwrap();
        let drag = content
            .observe_controllers()
            .snapshot()
            .into_iter()
            .find_map(|controller| controller.downcast::<gtk::GestureDrag>().ok())
            .unwrap();
        let initial_height = description_scroll.height();
        let initial_window_height = window.height();
        let bounds = handle.compute_bounds(&content).unwrap();
        let handle_y = f64::from(bounds.y()) + 1.0;
        drag.emit_by_name::<()>("drag-begin", &[&1.0f64, &handle_y]);
        drag.emit_by_name::<()>("drag-update", &[&0.0f64, &80.0f64]);
        drag.emit_by_name::<()>("drag-end", &[&0.0f64, &80.0f64]);
        drain();
        assert_eq!(description_scroll.height(), initial_height + 80);
        assert!(window.height() > initial_window_height);
        editor.sections[2].toggle.set_active(false);
        drain();
        editor.sections[2].toggle.set_active(true);
        drain();
        assert_eq!(description_scroll.height(), initial_height + 80);
        let handle_y = f64::from(handle.compute_bounds(&content).unwrap().y()) + 1.0;
        drag.emit_by_name::<()>("drag-begin", &[&1.0f64, &handle_y]);
        drag.emit_by_name::<()>("drag-update", &[&0.0f64, &-10000.0f64]);
        drag.emit_by_name::<()>("drag-end", &[&0.0f64, &-10000.0f64]);
        drain();
        assert_eq!(description_scroll.height(), initial_height);
        editor.sections[2].toggle.set_active(false);
        drain();

        editor.sections[0].toggle.set_active(true);
        drain();
        assert!(window.height() > collapsed_height);
        editor.start.entry.grab_focus();
        drain();
        let popup = editor
            .start
            .calendar
            .ancestor(gtk::Popover::static_type())
            .and_downcast::<gtk::Popover>()
            .unwrap();
        assert!(popup.is_mapped());
        assert!(press(gdk::Key::Right, gdk::ModifierType::CONTROL_MASK));
        assert_eq!(editor.start.entry.text(), "2026-10-16");
        assert!(press(gdk::Key::Escape, gdk::ModifierType::empty()));
        assert!(!popup.is_visible());
        assert!(window.is_visible());
        assert_eq!(saves.get(), 2);

        editor.location.grab_focus();
        editor.location.set_text("Off");
        drain();
        assert!(super::has_nested_popup(&window));
        assert!(!press(gdk::Key::Escape, gdk::ModifierType::empty()));
        let completion_keys = editor
            .location
            .observe_controllers()
            .snapshot()
            .into_iter()
            .find_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
            .unwrap();
        assert!(completion_keys.emit_by_name::<bool>(
            "key-pressed",
            &[&gdk::Key::Escape, &0u32, &gdk::ModifierType::empty(),]
        ));
        drain();
        assert!(!super::has_nested_popup(&window));
        assert_eq!(editor.location.text(), "Off");
        assert!(window.is_visible());
        for section in &editor.sections {
            section.toggle.set_active(true);
        }
        editor.form.set_size_request(-1, 2000);
        drain();
        let adjustment = editor.scroll.vadjustment();
        assert!(adjustment.upper() > adjustment.page_size());
        let bounds = editor.save.compute_bounds(&window).unwrap();
        assert!(bounds.y() >= 0.0);
        assert!(bounds.y() + bounds.height() <= window.height() as f32);
        assert!(editor.scroll.height() < editor.form.height());
        editor.form.set_size_request(-1, -1);
        for section in &editor.sections {
            section.toggle.set_active(false);
        }
        editor.summary.grab_focus();
        drain();
        assert_eq!(window.height(), collapsed_height);
        window.destroy();
        parent.close();
        drain();
    }
}
