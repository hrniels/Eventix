// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, rc::Rc};

use eventix_view::sidebar::SidebarSnapshot;
use gtk::{glib, prelude::*, subclass::prelude::*};

use super::popover::DetailsController;
use crate::style;

mod panel;
mod quick_task;

use panel::Panel;
use quick_task::QuickTask;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Sidebar {
        pub(super) quick_task: RefCell<Option<Rc<QuickTask>>>,
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
            let quick_task = QuickTask::new(locale.as_ref());
            tasks.content.append(&quick_task.widget);
            self.quick_task.replace(Some(quick_task));
            events.title.set_text(locale.translate("Next Events"));
            tasks.title.set_text(locale.translate("Next Tasks"));
            self.panels.replace(Some((events, tasks)));
        }

        fn dispose(&self) {
            self.details.borrow().close();
            self.panels.take();
            self.quick_task.take();
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
    pub(crate) fn quick_task(&self) -> Rc<QuickTask> {
        self.imp().quick_task.borrow().as_ref().unwrap().clone()
    }

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
        self.quick_task().update(
            &snapshot.task_calendars,
            snapshot.last_task_calendar.as_deref(),
            snapshot.locale.as_ref(),
        );
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
