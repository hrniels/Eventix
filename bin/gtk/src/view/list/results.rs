// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use eventix_locale::Locale;
use eventix_view::list::{Row, Snapshot};
use gtk::{glib, prelude::*};

use crate::view::indicators::EventIndicators;
use crate::view::response_marker;

use super::{item::ListItem, model::ListModel, row::FixedRow};
use crate::view::action_menu::{self as actions, Actions};

const ICON_SIZE: i32 = 20;

/// Renders a virtualized list with presentation data loaded on demand.
pub(super) struct Results {
    pub scroll: gtk::ScrolledWindow,
    pub model: ListModel,
    pub widget: gtk::Stack,
    pub actions: Rc<Actions<Row>>,
    empty: gtk::Label,
    locale: Rc<RefCell<Arc<dyn Locale + Send + Sync>>>,
}

impl Results {
    pub fn new() -> Self {
        let model: ListModel = glib::Object::new();
        let locale = Rc::new(RefCell::new(eventix_locale::default()));
        let actions = Rc::new(Actions::default());
        let factory = factory(locale.clone(), actions.clone());
        let selection = gtk::NoSelection::new(Some(model.clone()));
        let list = gtk::ListView::new(Some(selection), Some(factory));
        list.set_enable_rubberband(false);
        let empty = gtk::Label::new(None);
        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list)
            .build();
        let widget = gtk::Stack::new();
        widget.set_vexpand(true);
        widget.add_child(&scroll);
        widget.add_child(&empty);
        Self {
            scroll,
            model,
            widget,
            actions,
            empty,
            locale,
        }
    }

    pub fn reset_scroll(&self) {
        self.scroll.vadjustment().set_value(0.0);
    }

    pub fn update(&self, snapshot: Snapshot) {
        self.empty
            .set_text(snapshot.locale.translate("No items found"));
        self.locale.replace(snapshot.locale);
        let empty = snapshot.items.is_empty();
        self.model.set_items(snapshot.items);
        if empty {
            self.widget.set_visible_child(&self.empty);
        } else {
            self.widget.set_visible_child(&self.scroll);
        }
    }
}

fn factory(
    locale: Rc<RefCell<Arc<dyn Locale + Send + Sync>>>,
    actions: Rc<Actions<Row>>,
) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    let bindings = Rc::new(RefCell::new(HashMap::<
        gtk::ListItem,
        (ListItem, glib::SignalHandlerId),
    >::new()));
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        item.set_activatable(false);
        item.set_selectable(false);
        item.set_child(Some(&FixedRow::new()));
    });
    let bound = bindings.clone();
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let data = item.item().and_downcast::<ListItem>().unwrap();
        let row = item.child().and_downcast::<FixedRow>().unwrap();
        update_row(&row, &data, &**locale.borrow(), &actions);
        let weak = row.downgrade();
        let locale = locale.clone();
        let actions = actions.clone();
        let handler = data.connect_notify_local(Some("loaded"), move |data, _| {
            if let Some(row) = weak.upgrade() {
                update_row(&row, data, &**locale.borrow(), &actions);
            }
        });
        bound.borrow_mut().insert(item.clone(), (data, handler));
    });
    factory.connect_unbind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        if let Some((data, handler)) = bindings.borrow_mut().remove(item) {
            data.disconnect(handler);
        }
        if let Some(row) = item.child().and_downcast::<FixedRow>() {
            row.set_child(&gtk::Box::new(gtk::Orientation::Horizontal, 0));
        }
    });
    factory
}

fn update_row(widget: &FixedRow, item: &ListItem, locale: &dyn Locale, actions: &Rc<Actions<Row>>) {
    if let Some(row) = item.row().as_ref() {
        widget.set_child(&row_content(row, locale, actions));
    } else {
        widget.set_child(&gtk::Box::new(gtk::Orientation::Horizontal, 0));
    }
}

fn row_content(row: &Row, locale: &dyn Locale, handlers: &Rc<Actions<Row>>) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.add_css_class("ev_list_row");
    let kind = gtk::Image::from_icon_name(if row.task {
        "view-task-symbolic"
    } else {
        "x-office-calendar-symbolic"
    });
    kind.set_pixel_size(ICON_SIZE);
    kind.set_tooltip_text(Some(locale.translate(if row.task {
        "Task"
    } else {
        "Event"
    })));
    content.append(&kind);
    content.append(&response_marker::new(
        &row.background,
        &row.calendar_name,
        row.response.map(Into::into),
        ICON_SIZE,
        locale,
    ));
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
        completed.set_pixel_size(ICON_SIZE);
        completed.set_tooltip_text(Some(locale.translate("Completed")));
        content.append(&completed);
    }
    let indicators: EventIndicators = gtk::glib::Object::new();
    indicators.set_valign(gtk::Align::Center);
    indicators.set_pixel_size(ICON_SIZE);
    indicators.set_state(row.recurrent, false, row.alarm, locale);
    content.append(&indicators);
    let dates = gtk::Box::new(gtk::Orientation::Vertical, 0);
    dates.set_valign(gtk::Align::Center);
    dates.set_size_request(260, -1);
    for text in [&row.date_range, &row.recurrence]
        .into_iter()
        .filter(|text| !text.is_empty())
    {
        let label = gtk::Label::new(Some(text));
        label.set_xalign(0.5);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(1);
        label.set_tooltip_text(Some(text));
        dates.append(&label);
    }
    content.append(&dates);
    let actions = actions::button(row, &row.actions, locale, &content, handlers);
    content.append(&actions);
    content
}
