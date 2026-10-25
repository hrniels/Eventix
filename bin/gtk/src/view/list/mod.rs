// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod filtering;
mod pagination;
mod results;

use std::{cell::RefCell, rc::Rc};

use eventix_view::list::{Filter, Snapshot};
use gtk::prelude::*;

use self::{filtering::Filtering, pagination::Pagination, results::Results};

/// Coordinates independent filter, result-list and pagination components.
pub(crate) struct ListView {
    pub widget: gtk::Box,
    filtering: Rc<Filtering>,
    results: Results,
    pagination: Pagination,
    applied: RefCell<Filter>,
}

impl ListView {
    pub fn new() -> Rc<Self> {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.add_css_class("ev_list_view");
        let filtering = Filtering::new();
        let results = Results::new();
        let pagination = Pagination::new();
        pagination.widget.set_margin_top(8);
        let locale = eventix_locale::default();
        filtering.update(&[], &Filter::default(), &*locale);
        pagination.update(1, 0, &*locale);
        widget.append(&filtering.widget);
        widget.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        widget.append(&results.scroll);
        widget.append(&pagination.widget);
        Rc::new(Self {
            widget,
            filtering,
            results,
            pagination,
            applied: RefCell::default(),
        })
    }

    pub fn filter(&self) -> Filter {
        self.applied.borrow().clone()
    }

    pub fn header(&self) -> &gtk::Box {
        &self.filtering.widget
    }

    pub fn connect_changed(self: &Rc<Self>, changed: impl Fn() + 'static) {
        let changed: Rc<dyn Fn()> = Rc::new(changed);
        let weak = Rc::downgrade(self);
        let notify = changed.clone();
        self.filtering.connect_changed(move || {
            if let Some(view) = weak.upgrade() {
                view.applied.replace(view.filtering.filter());
                view.results.reset_scroll();
                notify();
            }
        });
        let weak = Rc::downgrade(self);
        self.pagination.connect_changed(move |forward| {
            if let Some(view) = weak.upgrade() {
                let page = view.applied.borrow().page;
                view.applied.borrow_mut().page = if forward {
                    page.saturating_add(1)
                } else {
                    page.saturating_sub(1).max(1)
                };
                view.results.reset_scroll();
                changed();
            }
        });
    }

    pub fn set_snapshot(&self, snapshot: Snapshot) {
        self.filtering
            .update(&snapshot.calendars, &snapshot.filter, &*snapshot.locale);
        self.pagination
            .update(snapshot.filter.page, snapshot.total, &*snapshot.locale);
        self.results.update(&snapshot.rows, &*snapshot.locale);
        self.applied.replace(snapshot.filter);
    }

    pub fn set_loading(&self, loading: bool) {
        self.pagination.widget.set_sensitive(!loading);
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use eventix_view::list::{Calendar, Filter, Row, Snapshot};
    use gtk::prelude::*;

    use super::ListView;

    fn descendants<T: IsA<gtk::Widget> + StaticType>(widget: &gtk::Widget) -> Vec<T> {
        let mut found = Vec::new();
        if let Ok(item) = widget.clone().downcast::<T>() {
            found.push(item);
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            found.extend(descendants::<T>(&widget));
            child = widget.next_sibling();
        }
        found
    }

    fn snapshot(filter: Filter) -> Snapshot {
        Snapshot {
            filter,
            total: 14,
            locale: eventix_locale::default(),
            calendars: ["Work", "Home"]
                .map(|name| Calendar {
                    id: name.into(),
                    name: name.into(),
                    background: "#803366".into(),
                })
                .to_vec(),
            rows: (0..12)
                .map(|index| Row {
                    calendar: "Work".into(),
                    calendar_name: "Work".into(),
                    uid: format!("item-{index}"),
                    summary: format!("Item {index}"),
                    background: "#803366".into(),
                    task: index == 0,
                    completed: index == 0,
                    recurrent: index == 1,
                    alarm: index == 1,
                    response: None,
                    date_range: "2026-10-25".into(),
                    recurrence: String::new(),
                })
                .collect(),
        }
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn filters_pagination_and_refresh_keep_native_focus_and_row_identity() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        let view = ListView::new();
        let changes = Rc::new(RefCell::new(Vec::new()));
        let weak = Rc::downgrade(&view);
        let recorded = changes.clone();
        view.connect_changed(move || recorded.borrow_mut().push(weak.upgrade().unwrap().filter()));
        view.set_snapshot(snapshot(Filter::default()));
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(500)
            .child(&view.widget)
            .build();
        window.present();
        let drain = || {
            for _ in 0..10 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        drain();
        let search = descendants::<gtk::Entry>(view.widget.upcast_ref()).remove(0);
        assert!(search.allocated_width() <= 200);
        search.grab_focus();
        search.set_text("alpha beta OR gamma");
        drain();
        assert!(
            search.allocated_width() <= 200,
            "keywords do not widen the entry"
        );
        assert!(
            changes.borrow().is_empty(),
            "typing is a draft until applied"
        );
        search.emit_activate();
        assert_eq!(view.filter().keywords, "alpha beta OR gamma");
        assert_eq!(view.filter().page, 1);
        let toggles = descendants::<gtk::ToggleButton>(view.widget.upcast_ref());
        for button in &toggles {
            assert_eq!(button.allocated_height(), search.allocated_height());
            assert_eq!(button.allocated_width(), button.allocated_height());
        }
        toggles[0].set_active(false);
        assert_eq!(view.filter().calendars, ["Home"]);
        let count = changes.borrow().len();
        toggles[1].set_active(false);
        assert!(toggles[1].is_active());
        assert_eq!(
            changes.borrow().len(),
            count,
            "the final calendar remains selected"
        );
        let reset = descendants::<gtk::Button>(view.widget.upcast_ref())
            .into_iter()
            .find(|button| button.tooltip_text().as_deref() == Some("Reset"))
            .unwrap();
        reset.emit_clicked();
        assert!(search.text().is_empty());
        assert_eq!(view.filter().calendars, ["Work", "Home"]);
        view.set_snapshot(snapshot(view.filter()));
        let next = view
            .pagination
            .widget
            .last_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        next.emit_clicked();
        assert_eq!(view.filter().page, 2);
        view.set_snapshot(snapshot(view.filter()));
        assert!(!next.is_sensitive());
        let previous = view
            .pagination
            .widget
            .first_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        previous.emit_clicked();
        assert_eq!(view.filter().page, 1);
        view.set_snapshot(snapshot(view.filter()));
        drain();
        let rows = descendants::<gtk::ListBoxRow>(view.widget.upcast_ref());
        assert_eq!(rows.len(), 12);
        assert!(rows.iter().all(|row| !row.is_activatable()));
        assert!(rows[0].grab_focus());
        view.results.scroll.vadjustment().set_value(50.0);
        let mut refreshed = snapshot(view.filter());
        refreshed.rows.reverse();
        view.set_snapshot(refreshed);
        drain();
        let updated = descendants::<gtk::ListBoxRow>(view.widget.upcast_ref());
        assert_eq!(updated[11], rows[0]);
        assert!(rows[0].has_focus());
        assert_eq!(view.results.scroll.vadjustment().value(), 50.0);
        let mut empty = snapshot(view.filter());
        empty.rows.clear();
        empty.total = 0;
        view.set_snapshot(empty);
        drain();
        assert!(descendants::<gtk::ListBoxRow>(view.widget.upcast_ref()).is_empty());
        assert!(!next.is_sensitive() && !previous.is_sensitive());
        window.close();
        drain();
    }
}
