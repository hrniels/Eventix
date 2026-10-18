// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};

use chrono::NaiveDate;
use eventix_locale::{DateFlags, Locale};
use eventix_view::{
    OccurrenceId,
    details::Response,
    sidebar::{SidebarGroup, SidebarRow, SidebarSnapshot},
};
use gtk::{glib, prelude::*, subclass::prelude::*};

use super::{details::EventDetails, popover::DetailsController};
use crate::style;

type RowKey = (Option<NaiveDate>, OccurrenceId);

struct Panel {
    header: gtk::Box,
    title: gtk::Label,
    new_item: gtk::Button,
    list: gtk::Box,
    scroll: gtk::ScrolledWindow,
    rows: BTreeMap<RowKey, gtk::Button>,
}

impl Panel {
    fn new(parent: &gtk::Box, is_events: bool) -> Self {
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        panel.set_vexpand(true);

        let frame = gtk::Frame::builder().child(&panel).vexpand(true).build();
        frame.add_css_class("ev_panel_frame");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        header.add_css_class("ev_panel_header");
        let title = gtk::Label::new(None);
        title.set_hexpand(true);
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.add_css_class("ev_panel_title");
        header.append(&title);
        let new_item = {
            let button = gtk::Button::from_icon_name("list-add-symbolic");
            button.set_action_name(Some(if is_events {
                "win.new-event"
            } else {
                "win.new-task"
            }));
            button.add_css_class("ev_sidebar_add_item");
            header.append(&button);
            button
        };
        panel.append(&header);
        panel.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Always)
            .vexpand(true)
            .child(&list)
            .build();

        panel.append(&scroll);
        parent.append(&frame);

        Self {
            header,
            title,
            new_item,
            list,
            scroll,
            rows: BTreeMap::new(),
        }
    }

    fn update(
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
                let button = self.rows.remove(&key).unwrap_or_default();
                update_row(&button, row);
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
    let label = gtk::Label::builder()
        .label(&heading)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    label.add_css_class("ev_sidebar_day");
    label
}

fn update_row(button: &gtk::Button, row: &SidebarRow) {
    for class in button.css_classes() {
        button.remove_css_class(&class);
    }
    button.add_css_class("ev_sidebar_row");
    button.set_child(Some(&row_content(row)));
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

fn row_content(row: &SidebarRow) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    if !row.full_day || row.task {
        let marker = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        marker.set_valign(gtk::Align::Center);
        marker.add_css_class("ev_sidebar_marker");
        marker.add_css_class(&style::calendar_class(&row.background, &row.foreground));
        if let Some(response) = row.response {
            let (name, class) = match response {
                Response::Accepted => ("check-lg", "ev_details_good"),
                Response::Declined => ("x-lg", "ev_details_critical"),
                Response::Tentative => ("question-lg", "ev_details_warn"),
                Response::Pending => ("", ""),
            };
            if !name.is_empty() {
                let image = icon(name);
                image.add_css_class("ev_sidebar_response");
                image.add_css_class(class);
                marker.append(&image);
            }
        }
        content.append(&marker);
    }
    if !row.time.is_empty() {
        let time = gtk::Label::new(Some(&row.time));
        time.add_css_class("ev_sidebar_time");
        content.append(&time);
    }
    let summary = gtk::Label::builder()
        .label(&row.summary)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .hexpand(true)
        .build();
    content.append(&summary);
    for (visible, name) in [
        (row.recurrent, "arrow-clockwise"),
        (row.exception, "pencil"),
        (row.alarm, "alarm"),
    ] {
        if visible {
            content.append(&icon(name));
        }
    }
    content
}

fn icon(name: &str) -> gtk::Image {
    let image = gtk::Image::from_resource(&format!(
        "/io/github/hrniels/Eventix/gtk/icons/{name}-symbolic.svg"
    ));
    image.set_pixel_size(14);
    image
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Sidebar {
        pub(super) panels: RefCell<Option<(Panel, Panel)>>,
        pub(super) details: RefCell<Rc<DetailsController>>,
        pub colors: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Sidebar {
        const NAME: &'static str = "EventixSidebar";
        type Type = super::Sidebar;
        type ParentType = gtk::Box;
    }
    impl ObjectImpl for Sidebar {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_orientation(gtk::Orientation::Vertical);
            obj.set_homogeneous(true);
            obj.set_spacing(12);
            obj.add_css_class("ev_sidebar");
            let events = Panel::new(obj.upcast_ref(), true);
            let tasks = Panel::new(obj.upcast_ref(), false);
            let locale = eventix_locale::default();
            events.title.set_text(locale.translate("Next Events"));
            tasks.title.set_text(locale.translate("Next Tasks"));
            self.panels.replace(Some((events, tasks)));
        }
        fn dispose(&self) {
            self.details.borrow().close();
            self.panels.take();
        }
    }
    impl WidgetImpl for Sidebar {}
    impl BoxImpl for Sidebar {}
}

glib::wrapper! {
    pub struct Sidebar(ObjectSubclass<imp::Sidebar>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl Sidebar {
    pub(crate) fn set_details(&self, details: Rc<DetailsController>) {
        self.imp().details.replace(details);
    }

    /// Returns the title labels of both panels so callers can align header heights with them.
    pub(crate) fn panel_titles(&self) -> Vec<gtk::Widget> {
        self.imp()
            .panels
            .borrow()
            .as_ref()
            .map(|(events, tasks)| {
                vec![
                    events.header.clone().upcast(),
                    tasks.header.clone().upcast(),
                ]
            })
            .unwrap_or_default()
    }

    pub(crate) fn new_item_button(&self, kind: eventix_ical::objects::CalCompType) -> gtk::Button {
        self.imp()
            .panels
            .borrow()
            .as_ref()
            .map(|(events, tasks)| {
                if kind == eventix_ical::objects::CalCompType::Event {
                    events
                } else {
                    tasks
                }
                .new_item
                .clone()
            })
            .unwrap_or_default()
    }

    pub fn set_snapshot(&self, snapshot: SidebarSnapshot) {
        let imp = self.imp();
        let colors = snapshot
            .events
            .iter()
            .chain(&snapshot.tasks)
            .flat_map(|group| &group.rows)
            .map(|row| (row.background.as_str(), row.foreground.as_str()));
        imp.colors.replace(style::colors_css(colors));
        if let Some((events, tasks)) = imp.panels.borrow_mut().as_mut() {
            events
                .title
                .set_text(snapshot.locale.translate("Next Events"));
            events
                .new_item
                .set_tooltip_text(Some(snapshot.locale.translate("New Event")));
            tasks
                .new_item
                .set_tooltip_text(Some(snapshot.locale.translate("New Task")));
            tasks
                .title
                .set_text(snapshot.locale.translate("Next Tasks"));
            events.update(&snapshot.events, &snapshot.locale, &imp.details.borrow());
            tasks.update(&snapshot.tasks, &snapshot.locale, &imp.details.borrow());
        }
    }

    pub fn colors_css(&self) -> String {
        self.imp().colors.borrow().clone()
    }
}
