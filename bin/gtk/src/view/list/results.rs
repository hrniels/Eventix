// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, collections::BTreeMap};

use eventix_ical::objects::CalPartStat;
use eventix_locale::Locale;
use eventix_view::list::Row;
use gtk::prelude::*;

use crate::view::calendar_selector::set_dot_color;
use crate::view::indicators::EventIndicators;

type RowKey = (String, String);

/// Renders the result list and retains live row identity on refresh.
pub(super) struct Results {
    pub scroll: gtk::ScrolledWindow,
    list: gtk::ListBox,
    empty: gtk::Label,
    rows: RefCell<BTreeMap<RowKey, gtk::ListBoxRow>>,
}

impl Results {
    pub fn new() -> Self {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        let empty = gtk::Label::new(None);
        list.set_placeholder(Some(&empty));
        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list)
            .build();
        Self {
            scroll,
            list,
            empty,
            rows: RefCell::default(),
        }
    }

    pub fn reset_scroll(&self) {
        self.scroll.vadjustment().set_value(0.0);
    }

    pub fn update(&self, rows: &[Row], locale: &dyn Locale) {
        self.empty.set_text(locale.translate("No items found"));
        let mut old = self.rows.take();
        let mut live = BTreeMap::new();
        for (index, row) in rows.iter().enumerate() {
            let key = (row.calendar.clone(), row.uid.clone());
            let widget = old.remove(&key).unwrap_or_else(|| {
                gtk::ListBoxRow::builder()
                    .activatable(false)
                    .selectable(false)
                    .focusable(true)
                    .build()
            });
            widget.set_child(Some(&row_content(row, locale)));
            if widget.parent().is_none() {
                self.list.insert(&widget, index as i32);
            } else if widget.index() != index as i32 {
                let focused = widget.has_focus();
                self.list.remove(&widget);
                self.list.insert(&widget, index as i32);
                if focused {
                    widget.grab_focus();
                }
            }
            live.insert(key, widget);
        }
        for widget in old.into_values() {
            self.list.remove(&widget);
        }
        self.rows.replace(live);
    }
}

fn row_content(row: &Row, locale: &dyn Locale) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.add_css_class("ev_list_row");
    let kind = gtk::Image::from_icon_name(if row.task {
        "view-task-symbolic"
    } else {
        "x-office-calendar-symbolic"
    });
    kind.set_tooltip_text(Some(locale.translate(if row.task {
        "Task"
    } else {
        "Event"
    })));
    content.append(&kind);
    let dot = gtk::DrawingArea::new();
    dot.set_size_request(16, 16);
    dot.set_tooltip_text(Some(&row.calendar_name));
    let color = gtk::gdk::RGBA::parse(&row.background).unwrap_or(gtk::gdk::RGBA::TRANSPARENT);
    set_dot_color(&dot, color);
    content.append(&dot);
    let summary = gtk::Label::new(Some(&row.summary));
    summary.set_xalign(0.0);
    summary.set_hexpand(true);
    summary.set_ellipsize(gtk::pango::EllipsizeMode::End);
    summary.set_tooltip_text(Some(&row.summary));
    if row.completed {
        summary.add_css_class("ev_list_completed");
    }
    content.append(&summary);
    if row.completed {
        let completed = gtk::Image::from_icon_name("emblem-ok-symbolic");
        completed.set_tooltip_text(Some(locale.translate("Completed")));
        content.append(&completed);
    }
    if let Some(response) = row.response {
        let (icon, title) = match response {
            CalPartStat::Accepted => ("emblem-ok-symbolic", "Accepted"),
            CalPartStat::Tentative => ("dialog-question-symbolic", "Tentative"),
            CalPartStat::Declined => ("window-close-symbolic", "Declined"),
            _ => ("dialog-question-symbolic", "NEEDS-ACTION"),
        };
        let response = gtk::Image::from_icon_name(icon);
        response.set_tooltip_text(Some(locale.translate(title)));
        content.append(&response);
    }
    let indicators: EventIndicators = gtk::glib::Object::new();
    indicators.set_state(row.recurrent, false, row.alarm, locale);
    content.append(&indicators);
    let dates = gtk::Box::new(gtk::Orientation::Vertical, 2);
    dates.set_valign(gtk::Align::Center);
    dates.set_size_request(260, -1);
    for text in [&row.date_range, &row.recurrence]
        .into_iter()
        .filter(|text| !text.is_empty())
    {
        let label = gtk::Label::new(Some(text));
        label.set_xalign(0.5);
        label.set_wrap(true);
        dates.append(&label);
    }
    content.append(&dates);
    content
}
