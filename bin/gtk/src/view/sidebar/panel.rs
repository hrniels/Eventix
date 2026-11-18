// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::BTreeMap, rc::Rc, sync::Arc};

use chrono::NaiveDate;
use eventix_locale::{DateFlags, Locale};
use gtk::prelude::*;

use super::super::{
    details::EventDetails, indicators::EventIndicators, popover::DetailsController, response_marker,
};
use crate::model::{
    OccurrenceId,
    sidebar::{SidebarGroup, SidebarRow},
};
use crate::style;

type RowKey = (Option<NaiveDate>, OccurrenceId);

pub(super) struct Panel {
    pub(super) content: gtk::Box,
    pub(super) header: gtk::Box,
    pub(super) title: gtk::Label,
    pub(super) new_item: gtk::Button,
    list: gtk::Box,
    scroll: gtk::ScrolledWindow,
    rows: BTreeMap<RowKey, gtk::Button>,
}

impl Panel {
    pub(super) fn new(parent: &gtk::Box, is_events: bool) -> Self {
        let builder = gtk::Builder::from_resource(resource_path!("view/sidebar/panel.ui"));
        let new_item: gtk::Button = builder.object("new_item").unwrap();
        new_item.set_action_name(Some(if is_events {
            "win.new-event"
        } else {
            "win.new-task"
        }));
        parent.append(&builder.object::<gtk::Frame>("frame").unwrap());

        Self {
            content: builder.object("content").unwrap(),
            header: builder.object("header").unwrap(),
            title: builder.object("title").unwrap(),
            new_item,
            list: builder.object("list").unwrap(),
            scroll: builder.object("scroll").unwrap(),
            rows: BTreeMap::new(),
        }
    }

    pub(super) fn update(
        &mut self,
        groups: &[SidebarGroup],
        locale: &Arc<dyn Locale + Send + Sync>,
        details: &Rc<DetailsController>,
    ) {
        let focused = self
            .rows
            .iter()
            .find(|(_, button)| button.has_focus())
            .map(|(key, _)| key.clone());
        let position = self.scroll.vadjustment().value();

        self.remove_headings();
        let rows = self.update_rows(groups, locale, details);
        self.remove_unused_rows(details);
        self.rows = rows;

        self.scroll.vadjustment().set_value(position);
        if let Some(button) = focused.and_then(|key| self.rows.get(&key)) {
            button.grab_focus();
        }
    }

    fn remove_headings(&self) {
        let mut child = self.list.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if !widget.is::<gtk::Button>() {
                self.list.remove(&widget);
            }
        }
    }

    /// Updates displayed rows and returns their buttons, leaving obsolete buttons for removal.
    fn update_rows(
        &mut self,
        groups: &[SidebarGroup],
        locale: &Arc<dyn Locale + Send + Sync>,
        details: &Rc<DetailsController>,
    ) -> BTreeMap<RowKey, gtk::Button> {
        let mut rows = BTreeMap::new();
        let mut previous: Option<gtk::Widget> = None;
        for group in groups {
            let label = group_heading(group, &**locale);
            self.place_child(label.upcast_ref(), previous.as_ref());
            previous = Some(label.upcast());
            for row in &group.rows {
                let key = (group.date, row.id.clone());
                let button = self.rows.remove(&key).unwrap_or_else(|| {
                    gtk::Builder::from_resource(resource_path!("view/sidebar/row.ui"))
                        .object("row")
                        .unwrap()
                });
                update_row(&button, row, &**locale);
                bind_details_action(&button, row, locale, details);
                self.place_child(button.upcast_ref(), previous.as_ref());
                previous = Some(button.clone().upcast());
                rows.insert(key, button);
            }
        }

        rows
    }

    fn place_child(&self, child: &gtk::Widget, previous: Option<&gtk::Widget>) {
        if child.parent().is_none() {
            self.list.append(child);
        }

        self.list.reorder_child_after(child, previous);
    }

    fn remove_unused_rows(&self, details: &DetailsController) {
        for button in self.rows.values() {
            details.close_for(button.upcast_ref());
            self.list.remove(button);
        }
    }
}

fn group_heading(group: &SidebarGroup, locale: &dyn Locale) -> gtk::Label {
    let heading = group
        .date
        .map(|date| locale.fmt_weekdate(&date, DateFlags::empty()))
        .unwrap_or_else(|| locale.translate("Unplanned").to_owned());
    let builder = gtk::Builder::from_resource(resource_path!("view/sidebar/heading.ui"));
    let label: gtk::Label = builder.object("heading").unwrap();
    label.set_text(&heading);

    label
}

fn update_row(button: &gtk::Button, row: &SidebarRow, locale: &dyn Locale) {
    for class in button.css_classes() {
        if class != "ev_sidebar_row" {
            button.remove_css_class(&class);
        }
    }

    button.set_child(Some(&row_content(row, locale)));

    if row.full_day && !row.task {
        button.add_css_class("ev_sidebar_full_day");
        button.add_css_class(&style::calendar_class(&row.background, &row.foreground));
    }

    if row.overdue {
        button.add_css_class("ev_sidebar_overdue");
    }

    if row.due_today {
        button.add_css_class("ev_sidebar_due_today");
    }

    if row.cancelled {
        button.add_css_class("ev_cancelled");
    }

    button.set_tooltip_text(Some(&format!(
        "{}{}{}",
        row.time,
        if row.time.is_empty() { "" } else { " " },
        row.summary
    )));
}

/// Replaces the row's details action so reused buttons always open the latest snapshot.
fn bind_details_action(
    button: &gtk::Button,
    row: &SidebarRow,
    locale: &Arc<dyn Locale + Send + Sync>,
    details: &Rc<DetailsController>,
) {
    button.set_action_name(None);
    let snapshot = row.clone();
    let context_locale = locale.clone();
    super::super::action_menu::install_context(button, move || {
        let target = super::super::action_menu::OccurrenceTarget::new(
            &snapshot.id,
            &snapshot.summary,
            snapshot.recurrent,
            snapshot.task,
        )?;
        Some((target, snapshot.details.clone(), context_locale.clone()))
    });
    let actions = gtk::gio::SimpleActionGroup::new();
    let action = gtk::gio::SimpleAction::new("details", None);
    let controller = details.clone();
    let anchor = button.downgrade();
    let row = row.clone();
    let locale = locale.clone();
    action.connect_activate(move |_, _| {
        if let Some(button) = anchor.upgrade() {
            controller.show_details(
                button.upcast_ref(),
                button.upcast_ref(),
                EventDetails::new_sidebar(&row, &*locale),
            );
        }
    });
    actions.add_action(&action);
    button.insert_action_group("row", Some(&actions));
    button.set_action_name(Some("row.details"));
}

fn row_content(row: &SidebarRow, locale: &dyn Locale) -> gtk::Box {
    EventIndicators::ensure_type();
    let builder = gtk::Builder::from_resource(resource_path!("view/sidebar/row_content.ui"));
    let content: gtk::Box = builder.object("content").unwrap();
    if !row.full_day || row.task {
        content.prepend(&response_marker::new(
            &row.background,
            &row.calendar,
            row.response,
            18,
            locale,
        ));
    }

    let time: gtk::Label = builder.object("time").unwrap();
    time.set_text(&row.time);
    time.set_visible(!row.time.is_empty());

    let summary: gtk::Label = builder.object("summary").unwrap();
    summary.set_text(&row.summary);

    let indicators: EventIndicators = builder.object("indicators").unwrap();
    indicators.set_pixel_size(14);
    indicators.set_state(row.recurrent, row.exception, row.alarm, locale);

    content
}
