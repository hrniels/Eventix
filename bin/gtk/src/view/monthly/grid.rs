// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, rc::Rc};

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, Utc};
use chrono_tz::Tz;
use eventix_locale::TimeFlags;
use eventix_view::MonthSnapshot;
use gtk::{glib, graphene, prelude::*, subclass::prelude::*};

use crate::actions::items::move_event::{self, DraggedEvent, MoveTarget};
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

#[derive(PartialEq)]
struct DropPreview {
    start: DateTime<Tz>,
    end: DateTime<Tz>,
    all_day: bool,
}

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
        pub rows: RefCell<Vec<gtk::Overlay>>,
        pub scroll_fades: RefCell<Vec<(gtk::Box, gtk::Box)>>,
        pub(super) contents: RefCell<Vec<WeekContent>>,
        pub(super) drop_preview: RefCell<Option<DropPreview>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MonthGrid {
        const NAME: &'static str = "EventixMonthGrid";
        type Type = super::MonthGrid;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for MonthGrid {
        fn constructed(&self) {
            self.parent_constructed();
            move_event::install_target(&*self.obj());
        }

        fn dispose(&self) {
            self.details.borrow().close();
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
            self.weekdays.take();
            self.days.take();
            self.weeks.take();
            self.scrollers.take();
            self.rows.take();
            self.scroll_fades.take();
            self.contents.take();
        }
    }

    impl WidgetImpl for MonthGrid {
        fn unmap(&self) {
            self.obj().clear_drop_preview();
            self.parent_unmap();
        }

        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            if orientation == gtk::Orientation::Horizontal {
                (MIN_WIDTH, MIN_WIDTH, -1, -1)
            } else {
                let count = self.weeks.borrow().len() as i32;
                let header = self.obj().weekday_header_height();
                (header + count * MIN_ROW, header + count * MAX_ROW, -1, -1)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, _: i32) {
            let obj = self.obj();
            if self.month.borrow().is_none() {
                return;
            }
            let allocate = |widget: &gtk::Widget, x: i32, y: i32, w: i32, h: i32| {
                widget.measure(gtk::Orientation::Horizontal, -1);
                widget.measure(gtk::Orientation::Vertical, w);
                widget.size_allocate(&gtk::Allocation::new(x, y, w.max(1), h.max(1)), -1);
            };
            let header = obj.weekday_header_height();
            for (day, label) in self.weekdays.borrow().iter().enumerate() {
                let left = day_x(width, day as f64).round() as i32;
                let right = day_x(width, (day + 1) as f64).round() as i32;
                allocate(
                    label.upcast_ref(),
                    left + GAP,
                    0,
                    right - left - 2 * GAP,
                    header - 1,
                );
            }
            let mut y = header;
            let rows = self.rows.borrow();
            let labels = self.days.borrow();
            let weeks = self.weeks.borrow();
            for (row, row_height) in obj.allocated_rows(height).into_iter().enumerate() {
                allocate(
                    weeks[row].upcast_ref(),
                    GAP,
                    y + GAP,
                    GUTTER - 2 * GAP,
                    HEADER,
                );
                allocate(
                    rows[row].upcast_ref(),
                    GUTTER,
                    y + HEADER,
                    width - GUTTER,
                    row_height - HEADER - GAP,
                );
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
            let header = obj.weekday_header_height();
            let mut y = header;
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
                rect(
                    snapshot,
                    &line,
                    0.0,
                    (y - 1) as f32,
                    obj.width() as f32,
                    1.0,
                );
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
                    header as f32,
                    1.0,
                    (y - header) as f32,
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
            for row in self.rows.borrow().iter() {
                obj.snapshot_child(row, snapshot);
            }
            obj.snapshot_drop_preview(snapshot);
        }
    }
}

glib::wrapper! {
    pub struct MonthGrid(ObjectSubclass<imp::MonthGrid>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MonthGrid {
    pub(crate) fn drop_date(&self, x: f64, y: f64) -> Option<NaiveDate> {
        let index = drop_day(
            self.width(),
            self.weekday_header_height(),
            &self.allocated_rows(self.height()),
            x,
            y,
        )?;
        self.imp()
            .month
            .borrow()
            .as_ref()?
            .days
            .get(index)
            .map(|day| day.date)
    }

    fn snapshot_drop_preview(&self, snapshot: &gtk::Snapshot) {
        let preview = self.imp().drop_preview.borrow();
        let month = self.imp().month.borrow();
        let (
            Some(DropPreview {
                start,
                end,
                all_day,
            }),
            Some(month),
        ) = (preview.as_ref(), month.as_ref())
        else {
            return;
        };
        let text = if *all_day {
            month.locale.translate("All day").to_string()
        } else {
            format!(
                "{}–{}",
                month.locale.fmt_time(start, TimeFlags::Short),
                month.locale.fmt_time(end, TimeFlags::Short)
            )
        };
        let border = style::color(self, "ev_drop_border");
        let mut y = self.weekday_header_height() as f32;
        for (row, height) in self.allocated_rows(self.height()).into_iter().enumerate() {
            for col in 0..7 {
                let date = month.days[row * 7 + col].date;
                if !preview_contains(date, start.naive_local(), end.naive_local()) {
                    continue;
                }
                let x = day_x(self.width(), col as f64) as f32 + 2.0;
                let w = (day_x(self.width(), (col + 1) as f64) as f32 - x - 2.0).max(1.0);
                let h = height as f32 - 4.0;
                rect(
                    snapshot,
                    &style::color(self, "ev_drop_background"),
                    x,
                    y + 2.0,
                    w,
                    h,
                );
                rect(snapshot, &border, x, y + 2.0, w, 2.0);
                rect(snapshot, &border, x, y + h, w, 2.0);
                rect(snapshot, &border, x, y + 2.0, 2.0, h);
                rect(snapshot, &border, x + w - 2.0, y + 2.0, 2.0, h);
                // A solid label stays legible over existing events in busy day cells.
                let label_y = y + height as f32 - 30.0;
                rect(
                    snapshot,
                    &border,
                    x + 2.0,
                    label_y,
                    (w - 4.0).max(1.0),
                    26.0,
                );
                let layout = self.create_pango_layout(Some(&text));
                layout.set_width(((w - 12.0).max(0.0) * gtk::pango::SCALE as f32) as i32);
                layout.set_ellipsize(gtk::pango::EllipsizeMode::End);
                snapshot.save();
                snapshot.translate(&graphene::Point::new(x + 6.0, label_y + 3.0));
                snapshot.append_layout(&layout, &style::color(self, "ev_drop_text"));
                snapshot.restore();
            }
            y += height as f32;
        }
    }

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
        let available = (height - self.weekday_header_height()).max(0);
        (0..count)
            .map(|row| {
                (available / count + i32::from(row < available % count)).clamp(MIN_ROW, MAX_ROW)
            })
            .collect()
    }

    /// Matches the weekly labels' natural height, including the separator below them.
    fn weekday_header_height(&self) -> i32 {
        self.imp()
            .weekdays
            .borrow()
            .iter()
            .map(|label| label.measure(gtk::Orientation::Vertical, -1).1)
            .max()
            .unwrap_or(0)
            + 1
    }

    pub fn set_month(&self, month: Rc<MonthSnapshot>) {
        self.clear_drop_preview();
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
            for row in imp.rows.take() {
                row.unparent();
            }
            imp.scrollers.take();
            for (top, bottom) in imp.scroll_fades.take() {
                top.unparent();
                bottom.unparent();
            }
            imp.contents.take();
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
                    let builder = grid_children(&[
                        "week",
                        "row",
                        "scroll",
                        "scroll_top_fade",
                        "scroll_bottom_fade",
                    ]);
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
                    let row_widget: gtk::Overlay = builder
                        .object("row")
                        .expect("week row is defined in GTK resources");
                    let top_fade: gtk::Box = builder
                        .object("scroll_top_fade")
                        .expect("week top scroll fade is defined in GTK resources");
                    let bottom_fade: gtk::Box = builder
                        .object("scroll_bottom_fade")
                        .expect("week bottom scroll fade is defined in GTK resources");
                    let update_fades = {
                        let top_fade = top_fade.downgrade();
                        let bottom_fade = bottom_fade.downgrade();
                        move |adjustment: &gtk::Adjustment| {
                            let at_top = adjustment.value() <= adjustment.lower();
                            let at_bottom =
                                adjustment.value() + adjustment.page_size() >= adjustment.upper();
                            if let Some(fade) = top_fade.upgrade() {
                                fade.set_visible(!at_top);
                            }
                            if let Some(fade) = bottom_fade.upgrade() {
                                fade.set_visible(!at_bottom);
                            }
                        }
                    };
                    let adjustment = scroll.vadjustment();
                    adjustment.connect_changed(update_fades.clone());
                    adjustment.connect_value_changed(update_fades);
                    row_widget.set_parent(self);
                    imp.rows.borrow_mut().push(row_widget);
                    imp.scrollers.borrow_mut().push(scroll);
                    imp.scroll_fades.borrow_mut().push((top_fade, bottom_fade));
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

impl MoveTarget for MonthGrid {
    fn move_start(&self, payload: &DraggedEvent, x: f64, y: f64) -> Option<NaiveDateTime> {
        Some(payload.on_date(self.drop_date(x, y)?))
    }

    fn show_move_preview(&self, payload: &DraggedEvent, x: f64, y: f64) -> bool {
        let Some(start) = self.move_start(payload, x, y) else {
            return false;
        };
        let month = self.imp().month.borrow();
        let Some((start, end)) = month
            .as_ref()
            .and_then(|month| payload.preview_range(start, month.locale.timezone()))
        else {
            return false;
        };
        let preview = DropPreview {
            start,
            end,
            all_day: payload.all_day,
        };
        if self.imp().drop_preview.borrow().as_ref() != Some(&preview) {
            self.imp().drop_preview.replace(Some(preview));
            self.queue_draw();
        }
        true
    }

    fn clear_drop_preview(&self) {
        if self.imp().drop_preview.take().is_some() {
            self.queue_draw();
        }
    }
}

/// Finds a day cell, excluding the weekday header, week numbers, and unused space.
fn drop_day(width: i32, header: i32, rows: &[i32], x: f64, y: f64) -> Option<usize> {
    if width <= GUTTER
        || !x.is_finite()
        || !y.is_finite()
        || x < f64::from(GUTTER)
        || x >= f64::from(width)
        || y < f64::from(header)
    {
        return None;
    }
    let col = ((x - f64::from(GUTTER)) * 7.0 / f64::from(width - GUTTER)).floor() as usize;
    let mut bottom = header;
    for (row, height) in rows.iter().enumerate() {
        bottom += height;
        if y < f64::from(bottom) {
            return Some(row * 7 + col);
        }
    }
    None
}

fn preview_contains(date: NaiveDate, start: NaiveDateTime, end: NaiveDateTime) -> bool {
    date >= start.date()
        && (date < end.date()
            || (date == end.date() && (end.time() != chrono::NaiveTime::MIN || start == end)))
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

#[cfg(test)]
mod tests {
    use super::{GUTTER, drop_day, preview_contains};

    #[test]
    fn drop_cells_include_headers_and_padding_days_but_exclude_grid_chrome() {
        let width = GUTTER + 700;
        let rows = [100, 101, 100, 100, 100];
        assert_eq!(drop_day(width, 32, &rows, f64::from(GUTTER), 32.0), Some(0));
        assert_eq!(
            drop_day(width, 32, &rows, f64::from(width - 1), 532.0),
            Some(34)
        );
        assert_eq!(
            drop_day(width, 32, &rows, f64::from(GUTTER + 250), 132.0),
            Some(9)
        );
        for (x, y) in [
            (0.0, 40.0),
            (f64::from(width), 40.0),
            (100.0, 31.0),
            (100.0, 533.0),
            (f64::NAN, 40.0),
            (100.0, f64::NAN),
        ] {
            assert_eq!(drop_day(width, 32, &rows, x, y), None);
        }
    }

    #[test]
    fn preview_excludes_midnight_end_and_includes_zero_duration_events() {
        let start = "2026-10-14T23:00:00".parse().unwrap();
        let end = "2026-10-16T00:00:00".parse().unwrap();
        for (date, included) in [
            ("2026-10-13", false),
            ("2026-10-14", true),
            ("2026-10-15", true),
            ("2026-10-16", false),
        ] {
            assert_eq!(
                preview_contains(date.parse().unwrap(), start, end),
                included
            );
        }
        assert!(preview_contains(start.date(), start, start));
    }
}
