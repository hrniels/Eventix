// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::Cell, rc::Rc};

use eventix_locale::Locale;
use gtk::prelude::*;

use super::FormSection;

pub(super) fn create(form: &gtk::Box, locale: &dyn Locale) -> (gtk::TextView, FormSection) {
    let builder = gtk::Builder::from_resource(resource_path!("view/editor/description.ui"));
    let description = builder.object("description").expect("description input");
    let content: gtk::Box = builder.object("content").expect("description content");
    let scroll: gtk::ScrolledWindow = builder.object("scroll").expect("description scroller");
    let handle: gtk::Box = builder
        .object("resize_handle")
        .expect("description resize handle");
    handle.set_cursor_from_name(Some("ns-resize"));
    handle.set_tooltip_text(Some(locale.translate("Drag to resize description")));
    connect_resize(&content, &handle, &scroll);
    let section = FormSection::new(form, locale, "Description", &content, None);

    (description, section)
}

fn connect_resize(content: &gtk::Box, handle: &gtk::Box, scroll: &gtk::ScrolledWindow) {
    let initial_height = Rc::new(Cell::new(0));
    let drag = gtk::GestureDrag::new();
    drag.set_button(1);
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    let initial = initial_height.clone();
    let weak = scroll.downgrade();
    let weak_handle = handle.downgrade();
    drag.connect_drag_begin(move |drag, x, y| {
        let Some(handle) = weak_handle.upgrade() else {
            return;
        };
        let over_handle = drag
            .widget()
            .and_then(|content| handle.compute_bounds(&content));
        if !over_handle.is_some_and(|bounds| {
            bounds.contains_point(&gtk::graphene::Point::new(x as f32, y as f32))
        }) {
            drag.set_state(gtk::EventSequenceState::Denied);
            return;
        }
        if let Some(scroll) = weak.upgrade() {
            initial.set(scroll.min_content_height());
            drag.set_state(gtk::EventSequenceState::Claimed);
        }
    });
    let weak = scroll.downgrade();
    drag.connect_drag_update(move |_, _, offset| {
        if let Some(scroll) = weak.upgrade() {
            resize(&scroll, f64::from(initial_height.get()) + offset);
        }
    });
    // Use the stationary content's coordinates: the handle itself moves during resizing.
    content.add_controller(drag);
}

fn resize(scroll: &gtk::ScrolledWindow, height: f64) {
    let screen_height = scroll
        .native()
        .and_then(|native| native.surface())
        .and_then(|surface| surface.display().monitor_at_surface(&surface))
        .map_or(800, |monitor| monitor.geometry().height());
    // Keep the textbox useful without letting a single drag create an enormous form.
    scroll
        .set_min_content_height((height.round() as i32).clamp(100, (screen_height - 200).max(100)));
}

#[cfg(test)]
mod tests {
    use gtk::prelude::*;

    #[test]
    #[ignore = "requires a GTK display"]
    fn description_resize_is_drag_only_bounds_height_and_resets_for_new_editors() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let locale = eventix_locale::default();
        let form = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let (description, _) = super::create(&form, locale.as_ref());
        let scroll = description
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>()
            .unwrap();
        let handle = scroll.next_sibling().unwrap();
        assert!(!handle.is_focusable());
        assert!(
            handle
                .observe_controllers()
                .snapshot()
                .into_iter()
                .all(|controller| !controller.is::<gtk::EventControllerKey>())
        );
        super::resize(&scroll, -10000.0);
        assert_eq!(scroll.min_content_height(), 100);

        super::resize(&scroll, 10000.0);
        // Without a native surface, resizing uses the fallback screen height of 800.
        assert_eq!(scroll.min_content_height(), 600);
        let (fresh, _) = super::create(&form, locale.as_ref());
        let fresh_scroll = fresh
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>()
            .unwrap();
        assert_eq!(fresh_scroll.min_content_height(), 100);
        assert_eq!(scroll.min_content_height(), 600);
    }
}
