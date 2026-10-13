// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, rc::Rc};

use chrono::{Datelike, Utc};
use eventix_view::MonthSnapshot;
use gtk::{glib, prelude::*, subclass::prelude::*};

use crate::style;
use crate::view::{
    card::EventCard,
    geometry::{GUTTER, MIN_WIDTH, day_x, rect},
    popover::DetailsController,
};

use super::week_content::WeekContent;

const HEADER: i32 = 32;
const MIN_ROW: i32 = 100;
pub(crate) const MAX_ROW: i32 = 180;
const GAP: i32 = 4;
const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct MonthGrid {
        pub(super) details: RefCell<Rc<DetailsController>>,
        pub month: RefCell<Option<Rc<MonthSnapshot>>>,
        pub weekdays: RefCell<Vec<gtk::Label>>,
        pub days: RefCell<Vec<gtk::Label>>,
        pub weeks: RefCell<Vec<gtk::Button>>,
        pub scrollers: RefCell<Vec<gtk::ScrolledWindow>>,
        pub(super) contents: RefCell<Vec<WeekContent>>,
        pub scrollbars: RefCell<Vec<gtk::Scrollbar>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MonthGrid {
        const NAME: &'static str = "EventixMonthGrid";
        type Type = super::MonthGrid;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for MonthGrid {
        fn dispose(&self) {
            self.details.borrow().close();
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
            self.weekdays.take();
            self.days.take();
            self.weeks.take();
            self.scrollers.take();
            self.contents.take();
            self.scrollbars.take();
        }
    }

    impl WidgetImpl for MonthGrid {
        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            if orientation == gtk::Orientation::Horizontal {
                (MIN_WIDTH, MIN_WIDTH, -1, -1)
            } else {
                let count = self.weeks.borrow().len() as i32;
                (HEADER + count * MIN_ROW, HEADER + count * MAX_ROW, -1, -1)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, _: i32) {
            let obj = self.obj();
            if self.month.borrow().is_none() {
                return;
            }
            let rows = obj.allocated_rows(height);
            let allocate = |widget: &gtk::Widget, x: i32, y: i32, w: i32, h: i32| {
                widget.measure(gtk::Orientation::Horizontal, -1);
                widget.measure(gtk::Orientation::Vertical, w);
                widget.size_allocate(&gtk::Allocation::new(x, y, w.max(1), h.max(1)), -1);
            };
            for (day, label) in self.weekdays.borrow().iter().enumerate() {
                let left = day_x(width, day as f64).round() as i32;
                let right = day_x(width, (day + 1) as f64).round() as i32;
                allocate(
                    label.upcast_ref(),
                    left + GAP,
                    0,
                    right - left - 2 * GAP,
                    HEADER,
                );
            }
            let mut y = HEADER;
            let scrollers = self.scrollers.borrow();
            let labels = self.days.borrow();
            let weeks = self.weeks.borrow();
            for (row, row_height) in rows.into_iter().enumerate() {
                allocate(
                    weeks[row].upcast_ref(),
                    GAP,
                    y + GAP,
                    GUTTER - 2 * GAP,
                    HEADER,
                );
                allocate(
                    scrollers[row].upcast_ref(),
                    GUTTER,
                    y + HEADER,
                    width - GUTTER,
                    row_height - HEADER - GAP,
                );
                let scrollbars = self.scrollbars.borrow();
                let scrollbar = &scrollbars[row];
                if scrollbar.is_visible() {
                    let w = scrollbar.measure(gtk::Orientation::Horizontal, -1).1;
                    allocate(
                        scrollbar.upcast_ref(),
                        GUTTER - GAP - w,
                        y + HEADER,
                        w,
                        row_height - HEADER - GAP,
                    );
                }
                for col in 0..7 {
                    let index = row * 7 + col;
                    let left = day_x(width, col as f64).round() as i32;
                    let right = day_x(width, (col + 1) as f64).round() as i32;
                    let w = right - left - 2 * GAP;
                    allocate(
                        labels[index].upcast_ref(),
                        left + GAP,
                        y + GAP,
                        w,
                        HEADER - GAP,
                    );
                }
                y += row_height;
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let Some(month) = self.month.borrow().clone() else {
                return;
            };
            let today = Utc::now()
                .with_timezone(month.locale.timezone())
                .date_naive();
            let line = style::color(&*obj, "ev_grid_line");
            let mut y = HEADER;
            for (row, height) in obj.allocated_rows(obj.height()).into_iter().enumerate() {
                for col in 0..7 {
                    let day = &month.days[row * 7 + col];
                    if day.date == today {
                        let left = day_x(obj.width(), col as f64) as f32;
                        let right = day_x(obj.width(), (col + 1) as f64) as f32;
                        rect(
                            snapshot,
                            &style::color(&*obj, "ev_today_background"),
                            left,
                            y as f32,
                            right - left,
                            height as f32,
                        );
                    }
                }
                rect(snapshot, &line, 0.0, y as f32, obj.width() as f32, 1.0);
                y += height;
            }
            rect(
                snapshot,
                &line,
                0.0,
                (y - 1) as f32,
                obj.width() as f32,
                1.0,
            );
            for day in 0..=7 {
                rect(
                    snapshot,
                    &line,
                    day_x(obj.width(), day as f64) as f32,
                    HEADER as f32,
                    1.0,
                    (y - HEADER) as f32,
                );
            }
            for label in self
                .weekdays
                .borrow()
                .iter()
                .chain(self.days.borrow().iter())
            {
                obj.snapshot_child(label, snapshot);
            }
            for button in self.weeks.borrow().iter() {
                obj.snapshot_child(button, snapshot);
            }
            for scroller in self.scrollers.borrow().iter() {
                obj.snapshot_child(scroller, snapshot);
            }
            for scrollbar in self.scrollbars.borrow().iter() {
                if scrollbar.is_visible() {
                    obj.snapshot_child(scrollbar, snapshot);
                }
            }
        }
    }
}

glib::wrapper! {
    pub struct MonthGrid(ObjectSubclass<imp::MonthGrid>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MonthGrid {
    pub(crate) fn set_details(&self, details: Rc<DetailsController>) {
        self.imp().details.replace(details.clone());
        for content in self.imp().contents.borrow().iter() {
            content.set_details(details.clone());
        }
    }

    pub(crate) fn event_cards(&self) -> Vec<(i64, EventCard)> {
        self.imp()
            .contents
            .borrow()
            .iter()
            .flat_map(WeekContent::event_cards)
            .collect()
    }

    /// Distributes viewport height evenly across weeks, bounded independently of event counts.
    fn allocated_rows(&self, height: i32) -> Vec<i32> {
        let count = self.imp().weeks.borrow().len() as i32;
        if count == 0 {
            return Vec::new();
        }
        let available = (height - HEADER).max(0);
        (0..count)
            .map(|row| {
                (available / count + i32::from(row < available % count)).clamp(MIN_ROW, MAX_ROW)
            })
            .collect()
    }

    pub fn set_month(&self, month: Rc<MonthSnapshot>) {
        let imp = self.imp();
        let changed = imp
            .month
            .borrow()
            .as_ref()
            .is_none_or(|old| old.month != month.month);
        if changed {
            imp.details.borrow().close();
            for label in imp.weekdays.take().into_iter().chain(imp.days.take()) {
                label.unparent();
            }
            for button in imp.weeks.take() {
                button.unparent();
            }
            for scroller in imp.scrollers.take() {
                scroller.unparent();
            }
            imp.contents.take();
            for scrollbar in imp.scrollbars.take() {
                scrollbar.unparent();
            }
            for name in WEEKDAYS {
                let builder = grid_children(&["weekday"]);
                let label: gtk::Label = builder
                    .object("weekday")
                    .expect("weekday label is defined in GTK resources");
                label.set_text(month.locale.translate(name));
                label.set_parent(self);
                imp.weekdays.borrow_mut().push(label);
            }
            for day in &month.days {
                if day.date.weekday() == chrono::Weekday::Mon {
                    let builder = grid_children(&["week", "adjustment", "scroll", "scrollbar"]);
                    let button: gtk::Button = builder
                        .object("week")
                        .expect("week button is defined in GTK resources");
                    button.set_label(&day.date.iso_week().week().to_string());
                    button.set_action_target_value(Some(&day.date.to_string().to_variant()));
                    button.set_parent(self);
                    imp.weeks.borrow_mut().push(button);
                    let content: WeekContent = builder
                        .object("content")
                        .expect("week content is defined in GTK resources");
                    content.set_details(imp.details.borrow().clone());
                    let scroll: gtk::ScrolledWindow = builder
                        .object("scroll")
                        .expect("week scroller is defined in GTK resources");
                    let scrollbar: gtk::Scrollbar = builder
                        .object("scrollbar")
                        .expect("week scrollbar is defined in GTK resources");
                    let weak = scrollbar.downgrade();
                    scroll.vadjustment().connect_changed(move |adjustment| {
                        if let Some(scrollbar) = weak.upgrade() {
                            scrollbar.set_visible(adjustment.upper() > adjustment.page_size());
                        }
                    });
                    scrollbar.set_parent(self);
                    scroll.set_parent(self);
                    imp.scrollbars.borrow_mut().push(scrollbar);
                    imp.scrollers.borrow_mut().push(scroll);
                    imp.contents.borrow_mut().push(content);
                }
                let builder = grid_children(&["day"]);
                let label: gtk::Label = builder
                    .object("day")
                    .expect("day label is defined in GTK resources");
                label.add_css_class(if day.current_month {
                    "ev_cur_month"
                } else {
                    "ev_other_month"
                });
                label.set_parent(self);
                imp.days.borrow_mut().push(label);
            }
        }
        for (label, name) in imp.weekdays.borrow().iter().zip(WEEKDAYS) {
            label.set_text(month.locale.translate(name));
        }
        for (button, week) in imp.weeks.borrow().iter().zip(month.days.chunks(7)) {
            button.set_tooltip_text(Some(&format!(
                "{} · {}",
                month.locale.translate("Weekly View"),
                month
                    .locale
                    .fmt_weekdate(&week[0].date, eventix_locale::DateFlags::NoToday)
            )));
        }
        for (label, day) in imp.days.borrow().iter().zip(&month.days) {
            let text = if day.show_month {
                format!(
                    "{} {}",
                    month.locale.translate(&day.date.format("%b").to_string()),
                    day.date.day()
                )
            } else {
                day.date.day().to_string()
            };
            label.set_text(&text);
        }
        for ((content, days), spans) in imp
            .contents
            .borrow()
            .iter()
            .zip(month.days.chunks(7))
            .zip(&month.spans)
        {
            content.set_week(days, spans, month.locale.clone());
        }
        imp.month.replace(Some(month));
        self.update_today();
        self.queue_resize();
    }

    pub(crate) fn update_today(&self) {
        let imp = self.imp();
        if let Some(month) = imp.month.borrow().as_ref() {
            let today = Utc::now()
                .with_timezone(month.locale.timezone())
                .date_naive();
            for (label, day) in imp.days.borrow().iter().zip(&month.days) {
                if day.date == today {
                    label.add_css_class("ev_today");
                } else {
                    label.remove_css_class("ev_today");
                }
            }
        }
        self.queue_draw();
    }
}

/// Instantiates the requested monthly grid objects.
fn grid_children(ids: &[&str]) -> gtk::Builder {
    WeekContent::ensure_type();
    let builder = gtk::Builder::new();
    builder
        .add_objects_from_resource("/io/github/hrniels/Eventix/gtk/view/monthly/grid.ui", ids)
        .expect("monthly grid children are defined in GTK resources");
    builder
}
