// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use eventix_locale::Locale;
use eventix_view::list::{Calendar, Filter};
use gtk::{gdk, prelude::*};

use crate::view::calendar_selector::set_dot_color;

type Changed = Rc<dyn Fn()>;

/// Owns draft keywords and the list's independent calendar selection.
pub(super) struct Filtering {
    pub widget: gtk::Box,
    search: gtk::Entry,
    calendars: gtk::Box,
    apply: gtk::Button,
    reset: gtk::Button,
    title: gtk::Label,
    choices: RefCell<Vec<(Calendar, gtk::ToggleButton)>>,
    updating: Cell<bool>,
    changed: RefCell<Option<Changed>>,
}

impl Filtering {
    pub fn new() -> Rc<Self> {
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        widget.add_css_class("ev_panel_header");
        let search = gtk::Entry::new();
        // Keep the natural entry width below the pixel request, regardless of font size.
        search.set_width_chars(1);
        search.set_max_width_chars(1);
        search.set_size_request(200, -1);
        let calendars = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let calendar_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hexpand(true)
            .child(&calendars)
            .build();
        let apply = gtk::Button::new();
        let reset = gtk::Button::from_icon_name("edit-clear-symbolic");
        let title = gtk::Label::new(None);
        title.set_halign(gtk::Align::End);
        title.add_css_class("ev_panel_title");
        widget.append(&search);
        widget.append(&apply);
        widget.append(&reset);
        widget.append(&calendar_scroll);
        widget.append(&title);
        let filtering = Rc::new(Self {
            widget,
            search,
            calendars,
            apply,
            reset,
            title,
            choices: RefCell::default(),
            updating: Cell::new(false),
            changed: RefCell::default(),
        });
        let weak = Rc::downgrade(&filtering);
        filtering.apply.connect_clicked(move |_| {
            if let Some(filtering) = weak.upgrade() {
                filtering.emit_changed();
            }
        });
        let weak = Rc::downgrade(&filtering);
        filtering.search.connect_activate(move |_| {
            if let Some(filtering) = weak.upgrade() {
                filtering.emit_changed();
            }
        });
        let weak = Rc::downgrade(&filtering);
        filtering.reset.connect_clicked(move |_| {
            if let Some(filtering) = weak.upgrade() {
                filtering.updating.set(true);
                filtering.search.set_text("");
                for (_, button) in filtering.choices.borrow().iter() {
                    button.set_active(true);
                }
                filtering.updating.set(false);
                filtering.emit_changed();
            }
        });
        filtering
    }

    pub fn connect_changed(&self, changed: impl Fn() + 'static) {
        self.changed.replace(Some(Rc::new(changed)));
    }

    fn emit_changed(&self) {
        if !self.updating.get()
            && let Some(changed) = self.changed.borrow().as_ref()
        {
            changed();
        }
    }

    pub fn filter(&self) -> Filter {
        Filter {
            keywords: self.search.text().to_string(),
            page: 1,
            calendars: self
                .choices
                .borrow()
                .iter()
                .filter(|(_, button)| button.is_active())
                .map(|(calendar, _)| calendar.id.clone())
                .collect(),
        }
    }

    pub fn update(self: &Rc<Self>, calendars: &[Calendar], filter: &Filter, locale: &dyn Locale) {
        self.updating.set(true);
        self.apply.set_label(locale.translate("Filter"));
        self.reset.set_tooltip_text(Some(locale.translate("Reset")));
        self.search
            .set_placeholder_text(Some(locale.translate("Search")));
        self.search
            .update_property(&[gtk::accessible::Property::Label(locale.translate("Search"))]);
        self.search.set_tooltip_text(Some(
            locale.translate(
                "Search summary, description, location or UID. Use OR for alternatives.",
            ),
        ));
        self.title.set_text(locale.translate("Events and Tasks"));
        let same = self
            .choices
            .borrow()
            .iter()
            .map(|(calendar, _)| calendar)
            .eq(calendars);
        if !same {
            while let Some(child) = self.calendars.first_child() {
                self.calendars.remove(&child);
            }
            self.choices.borrow_mut().clear();
            for calendar in calendars {
                let button = self.calendar_button(calendar);
                self.calendars.append(&button);
                self.choices.borrow_mut().push((calendar.clone(), button));
            }
        }
        // Do not replace draft keywords while an unrelated refresh is arriving.
        for (calendar, button) in self.choices.borrow().iter() {
            button
                .set_active(filter.calendars.is_empty() || filter.calendars.contains(&calendar.id));
        }
        self.updating.set(false);
    }

    fn calendar_button(self: &Rc<Self>, calendar: &Calendar) -> gtk::ToggleButton {
        let button = gtk::ToggleButton::new();
        button.add_css_class("ev_list_calendar");
        button.set_size_request(self.search.measure(gtk::Orientation::Vertical, -1).1, -1);
        button.set_tooltip_text(Some(&calendar.name));
        button.update_property(&[gtk::accessible::Property::Label(&calendar.name)]);
        let dot = gtk::DrawingArea::new();
        dot.set_size_request(16, 16);
        set_dot_color(
            &dot,
            gdk::RGBA::parse(&calendar.background).unwrap_or(gdk::RGBA::TRANSPARENT),
        );
        button.set_child(Some(&dot));

        let weak = Rc::downgrade(self);
        button.connect_toggled(move |button| {
            let Some(filtering) = weak.upgrade() else {
                return;
            };
            if filtering.updating.get() {
                return;
            }
            if !button.is_active()
                && filtering
                    .choices
                    .borrow()
                    .iter()
                    .all(|(_, button)| !button.is_active())
            {
                filtering.updating.set(true);
                button.set_active(true);
                filtering.updating.set(false);
                return;
            }
            filtering.emit_changed();
        });
        button
    }
}
