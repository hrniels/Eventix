// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;

use eventix_view::editor::EditorCalendar;
use gtk::{gdk, prelude::*};

#[derive(PartialEq, Eq)]
struct Choice {
    id: String,
    name: String,
    background: String,
}

/// Selects a calendar with a colored circle and a shared circle-and-name popup list.
pub(crate) struct CalendarSelector {
    pub widget: gtk::DropDown,
    choices: RefCell<Vec<Choice>>,
    compact: bool,
}

impl CalendarSelector {
    pub fn new(calendars: &[EditorCalendar], selected: Option<&str>, compact: bool) -> Self {
        let widget = gtk::DropDown::from_strings(&[]);
        widget.add_css_class("ev_calendar_selector");
        widget.connect_selected_notify(update_tooltip);
        let selector = Self {
            widget,
            choices: RefCell::default(),
            compact,
        };
        selector.update(calendars, selected);
        selector
    }

    /// Refreshes calendar choices while preserving the selected calendar when it still exists.
    pub fn update(&self, calendars: &[EditorCalendar], default: Option<&str>) {
        let choices: Vec<_> = calendars
            .iter()
            .map(|calendar| Choice {
                id: calendar.id.clone(),
                name: calendar.name.clone(),
                background: calendar.background.clone(),
            })
            .collect();
        if *self.choices.borrow() == choices {
            return;
        }

        let selected = self.selected_id();
        let selected = selected.as_deref().or(default);
        let index = choices
            .iter()
            .position(|choice| Some(choice.id.as_str()) == selected)
            .unwrap_or(0);
        let names: Vec<_> = choices.iter().map(|choice| choice.name.as_str()).collect();
        let model = gtk::StringList::new(&names).upcast::<gtk::gio::ListModel>();
        self.widget
            .set_factory(Some(&calendar_factory(&choices, &model, !self.compact)));
        self.widget
            .set_list_factory(Some(&calendar_factory(&choices, &model, true)));
        self.choices.replace(choices);
        self.widget.set_model(Some(&model));
        self.widget.set_selected(index as u32);
        update_tooltip(&self.widget);
    }

    pub fn selected_id(&self) -> Option<String> {
        self.choices
            .borrow()
            .get(self.widget.selected() as usize)
            .map(|choice| choice.id.clone())
    }
}

fn update_tooltip(widget: &gtk::DropDown) {
    let selected = widget.selected_item().and_downcast::<gtk::StringObject>();
    widget.set_tooltip_text(selected.as_ref().map(|item| item.string()).as_deref());
}

fn calendar_factory(
    choices: &[Choice],
    model: &gtk::gio::ListModel,
    show_name: bool,
) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };

        let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let dot = gtk::DrawingArea::new();
        dot.set_size_request(18, 18);
        content.append(&dot);
        if show_name {
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            content.append(&label);
        }
        item.set_child(Some(&content));
    });

    let colors: Vec<_> = choices
        .iter()
        .map(|choice| choice.background.clone())
        .collect();
    let model = model.clone();
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(content) = item.child() else {
            return;
        };
        if let Some(label) = content.last_child().and_downcast::<gtk::Label>()
            && let Some(text) = item.item().and_downcast::<gtk::StringObject>()
        {
            label.set_text(&text.string());
        }
        if let Some(dot) = content.first_child().and_downcast::<gtk::DrawingArea>() {
            // The collapsed dropdown uses a one-item model. Match the original object to
            // distinguish calendar colors even when several calendars have the same name.
            let position = (0..model.n_items()).find(|&index| model.item(index) == item.item());
            let color = position
                .and_then(|index| colors.get(index as usize))
                .and_then(|color| gdk::RGBA::parse(color).ok())
                .unwrap_or(gdk::RGBA::TRANSPARENT);
            set_dot_color(&dot, color);
        }
    });
    factory
}

fn set_dot_color(dot: &gtk::DrawingArea, color: gdk::RGBA) {
    dot.set_draw_func(move |_, cr, width, height| {
        cr.set_source_rgba(
            color.red().into(),
            color.green().into(),
            color.blue().into(),
            color.alpha().into(),
        );
        cr.arc(
            f64::from(width) / 2.0,
            f64::from(height) / 2.0,
            f64::from(width.min(height)) / 2.0 - 1.0,
            0.0,
            std::f64::consts::TAU,
        );
        let _ = cr.fill();
    });
}

#[cfg(test)]
mod tests {
    use eventix_view::editor::EditorCalendar;
    use gtk::prelude::*;

    use super::CalendarSelector;

    fn visible_children<T: IsA<gtk::Widget> + StaticType>(widget: &gtk::Widget) -> usize {
        if !widget.is_visible() || !widget.is_child_visible() {
            return 0;
        }
        let mut count = usize::from(widget.is::<T>());
        let mut child = widget.first_child();
        while let Some(widget) = child {
            count += visible_children::<T>(&widget);
            child = widget.next_sibling();
        }
        count
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn compact_and_full_selectors_preserve_calendar_identity_and_refresh_colors() {
        gtk::init().unwrap();
        for compact in [false, true] {
            let mut calendars = vec![
                EditorCalendar {
                    id: "red".into(),
                    name: "Same name".into(),
                    background: "#ff0000".into(),
                    personal: false,
                },
                EditorCalendar {
                    id: "blue".into(),
                    name: "Same name".into(),
                    background: "#0000ff".into(),
                    personal: false,
                },
            ];
            let selector = CalendarSelector::new(&calendars, Some("blue"), compact);
            let window = gtk::Window::builder().child(&selector.widget).build();
            window.present();
            while gtk::glib::MainContext::default().iteration(false) {}
            let button = selector.widget.first_child().unwrap();
            assert_eq!(visible_children::<gtk::DrawingArea>(&button), 1);
            assert_eq!(
                visible_children::<gtk::Label>(&button),
                usize::from(!compact)
            );
            assert_eq!(selector.selected_id().as_deref(), Some("blue"));

            let factory = selector.widget.factory().unwrap();
            selector.update(&calendars, Some("red"));
            assert_eq!(selector.widget.factory().unwrap(), factory);
            assert_eq!(selector.selected_id().as_deref(), Some("blue"));

            calendars.swap(0, 1);
            selector.update(&calendars, None);
            assert_eq!(selector.widget.selected(), 0);
            assert_eq!(selector.selected_id().as_deref(), Some("blue"));

            let factory = selector.widget.factory().unwrap();
            calendars[0].background = "#00ff00".into();
            selector.update(&calendars, None);
            assert_ne!(selector.widget.factory().unwrap(), factory);
            assert_eq!(selector.selected_id().as_deref(), Some("blue"));

            selector.widget.set_selected(1);
            assert_eq!(selector.selected_id().as_deref(), Some("red"));
            calendars[1].name = "Renamed".into();
            selector.update(&calendars, None);
            assert_eq!(selector.widget.tooltip_text().as_deref(), Some("Renamed"));

            calendars.pop();
            selector.update(&calendars, None);
            assert_eq!(selector.selected_id().as_deref(), Some("blue"));
            selector.update(&[], None);
            assert!(selector.selected_id().is_none());
            window.close();
        }
    }
}
