// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, rc::Rc};

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Timelike, Utc};
use chrono_tz::Tz;
use eventix_locale::TimeFlags;
use eventix_view::WeekSnapshot;
use gtk::{glib, graphene, prelude::*, subclass::prelude::*};

use crate::actions::items::move_event::DraggedEvent;
use crate::view::geometry::{GUTTER, MIN_WIDTH, day_x, draw_text, rect};
use crate::{
    style,
    view::{
        card::EventCard,
        popover::DetailsController,
        surface::{CardKey, Surface},
    },
};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct TimedGrid {
        pub(super) surface: Rc<Surface>,
        pub week: RefCell<Option<Rc<WeekSnapshot>>>,
        pub(super) drop_preview: RefCell<Option<(DateTime<Tz>, DateTime<Tz>)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TimedGrid {
        const NAME: &'static str = "EventixTimedGrid";
        type Type = super::TimedGrid;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for TimedGrid {
        fn constructed(&self) {
            self.parent_constructed();
            self.surface.construct(self.obj().upcast_ref());
            crate::actions::items::move_event::install_target(&self.obj());
        }

        fn dispose(&self) {
            self.surface.dispose(self.obj().upcast_ref());
        }
    }

    impl WidgetImpl for TimedGrid {
        fn map(&self) {
            self.parent_map();
            self.surface.map(self.obj().upcast_ref());
        }

        fn unmap(&self) {
            self.obj().clear_drop_preview();
            self.surface.unmap();
            self.parent_unmap();
        }

        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            let size = if orientation == gtk::Orientation::Horizontal {
                MIN_WIDTH
            } else {
                1440
            };
            (size, size, -1, -1)
        }

        fn size_allocate(&self, width: i32, _: i32, _: i32) {
            if let Some(week) = self.week.borrow().as_ref() {
                let segments =
                    week.days.iter().enumerate().flat_map(|(day, data)| {
                        data.timed.iter().map(move |segment| (day, segment))
                    });
                for (card, (day, segment)) in self.surface.cards.borrow().iter().zip(segments) {
                    let columns = segment.columns;
                    let y = segment.minute_start.min(1422) as i32;
                    let height = (segment.minute_end as i32 - y).max(18).min(1440 - y);
                    let left = day_x(
                        width,
                        day as f64 + columns.offset as f64 / columns.count as f64,
                    )
                    .round() as i32;
                    let right = day_x(
                        width,
                        day as f64 + (columns.offset + columns.span) as f64 / columns.count as f64,
                    )
                    .round() as i32;
                    card.button.size_allocate(
                        &gtk::Allocation::new(
                            left + 2,
                            y + 1,
                            (right - left - 4).max(1),
                            height - 2,
                        ),
                        -1,
                    );
                }
            }
            self.surface
                .details
                .borrow()
                .allocate(self.obj().upcast_ref());
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let width = obj.width() as f32;
            let height = obj.height() as f32;
            let day_width = (width - GUTTER as f32) / 7.0;
            let line = style::color(&*obj, "ev_grid_line");
            if let Some(week) = self.week.borrow().as_ref() {
                let now = Utc::now().with_timezone(week.locale.timezone());
                for (i, day) in week.days.iter().enumerate() {
                    let x = day_x(obj.width(), i as f64) as f32;
                    if day.date == now.date_naive() {
                        rect(
                            snapshot,
                            &style::color(&*obj, "ev_today_background"),
                            x,
                            0.0,
                            day_width,
                            height,
                        );
                    }
                    for range in &day.dst {
                        let color = style::color(
                            &*obj,
                            if range.fold {
                                "ev_dst_fold"
                            } else {
                                "ev_dst_gap"
                            },
                        );
                        rect(
                            snapshot,
                            &color,
                            x,
                            range.start as f32,
                            day_width,
                            (range.end - range.start) as f32,
                        );
                    }
                }
            }
            for day in 0..=7 {
                rect(
                    snapshot,
                    &line,
                    day_x(obj.width(), day as f64) as f32,
                    0.0,
                    1.0,
                    height,
                );
            }
            for hour in 0..24 {
                let y = hour as f32 * 60.0;
                rect(
                    snapshot,
                    &line,
                    GUTTER as f32,
                    y,
                    width - GUTTER as f32,
                    1.0,
                );
                draw_text(
                    obj.upcast_ref(),
                    snapshot,
                    &format!("{hour:02}:00"),
                    7.0,
                    y + 2.0,
                    52.0,
                );
            }
            self.surface.snapshot_cards(obj.upcast_ref(), snapshot);
            // Keep time changes visible over event cards, as in the HTML weekly view.
            if let Some(week) = self.week.borrow().as_ref() {
                for (i, day) in week.days.iter().enumerate() {
                    for range in &day.dst {
                        obj.snapshot_dst_overlay(snapshot, i, range);
                    }
                }
            }
            if let Some(week) = self.week.borrow().as_ref() {
                let now = Utc::now().with_timezone(week.locale.timezone());
                let day = (now.date_naive() - week.start).num_days();
                if (0..7).contains(&day) {
                    rect(
                        snapshot,
                        &style::color(&*obj, "ev_now"),
                        day_x(obj.width(), day as f64) as f32,
                        (now.hour() * 60 + now.minute()) as f32,
                        day_width,
                        2.0,
                    );
                }
            }
            obj.snapshot_drop_preview(snapshot);
        }
    }
}

glib::wrapper! {
    pub struct TimedGrid(ObjectSubclass<imp::TimedGrid>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl TimedGrid {
    fn snapshot_dst_overlay(
        &self,
        snapshot: &gtk::Snapshot,
        day: usize,
        range: &eventix_view::DstRange,
    ) {
        let x = day_x(self.width(), day as f64) as f32;
        let width = (self.width() - GUTTER) as f32 / 7.0;
        let y = range.start as f32;
        let height = (range.end - range.start) as f32;
        let mut color = self.color();

        snapshot.save();
        snapshot.push_clip(&graphene::Rect::new(x, y, width, height));
        snapshot.translate(&graphene::Point::new(x, y));
        snapshot.save();
        snapshot.rotate(-45.0);
        color.set_alpha(0.05);
        let mut stripe = -height;
        while stripe < width {
            rect(snapshot, &color, stripe, 0.0, 14.0, width + height);
            stripe += 28.0;
        }
        snapshot.restore();

        let layout = self.create_pango_layout(Some(if range.fold { "↑" } else { "↓" }));
        layout.set_font_description(Some(&gtk::pango::FontDescription::from_string("20")));
        let (text_width, text_height) = layout.pixel_size();
        snapshot.translate(&graphene::Point::new(
            (width - text_width as f32) / 2.0,
            (height - text_height as f32) / 2.0,
        ));
        color.set_alpha(0.5);
        snapshot.append_layout(&layout, &color);
        snapshot.pop();
        snapshot.restore();
    }

    fn snapshot_drop_preview(&self, snapshot: &gtk::Snapshot) {
        let preview = self.imp().drop_preview.borrow();
        let week = self.imp().week.borrow();
        let (Some((start, end)), Some(week)) = (preview.as_ref(), week.as_ref()) else {
            return;
        };
        for (i, day) in week.days.iter().enumerate() {
            let Some((y, height)) =
                preview_bounds(day.date, start.naive_local(), end.naive_local())
            else {
                continue;
            };
            let x = day_x(self.width(), i as f64) as f32 + 2.0;
            let w = (self.width() - GUTTER) as f32 / 7.0 - 4.0;
            let border = style::color(self, "ev_drop_border");
            rect(
                snapshot,
                &style::color(self, "ev_drop_background"),
                x,
                y,
                w,
                height,
            );
            rect(snapshot, &border, x, y, w, 2.0);
            rect(snapshot, &border, x, y + height - 2.0, w, 2.0);
            rect(snapshot, &border, x, y, 2.0, height);
            rect(snapshot, &border, x + w - 2.0, y, 2.0, height);
            let text = format!(
                "{}{}–{}{}",
                if start.date_naive() < day.date {
                    "← "
                } else {
                    ""
                },
                week.locale.fmt_time(start, TimeFlags::Short),
                week.locale.fmt_time(end, TimeFlags::Short),
                if end.date_naive() > day.date {
                    " →"
                } else {
                    ""
                },
            );
            let layout = self.create_pango_layout(Some(&text));
            layout.set_width(((w - 8.0).max(0.0) * gtk::pango::SCALE as f32) as i32);
            layout.set_ellipsize(gtk::pango::EllipsizeMode::End);
            snapshot.save();
            snapshot.push_clip(&graphene::Rect::new(x, y, w, height));
            snapshot.translate(&graphene::Point::new(x + 4.0, y + 2.0));
            snapshot.append_layout(&layout, &style::color(self, "ev_drop_text"));
            snapshot.pop();
            snapshot.restore();
        }
    }

    pub(crate) fn set_drop_preview(&self, payload: &DraggedEvent, x: f64, y: f64) -> bool {
        let Some(start) = self.drop_start(x, y, payload.offset_minutes) else {
            return false;
        };
        let week = self.imp().week.borrow();
        let Some(range) = week
            .as_ref()
            .and_then(|week| payload.preview_range(start, week.locale.timezone()))
        else {
            return false;
        };
        if self.imp().drop_preview.borrow().as_ref() != Some(&range) {
            self.imp().drop_preview.replace(Some(range));
            self.queue_draw();
        }
        true
    }

    pub(crate) fn clear_drop_preview(&self) {
        if self.imp().drop_preview.take().is_some() {
            self.queue_draw();
        }
    }

    pub(crate) fn set_resize_preview(&self, range: (DateTime<Tz>, DateTime<Tz>)) {
        self.imp().drop_preview.replace(Some(range));
        self.queue_draw();
    }

    pub(crate) fn drop_start(&self, x: f64, y: f64, offset: i64) -> Option<NaiveDateTime> {
        let week = self.imp().week.borrow();
        drop_start(week.as_ref()?.start, self.width(), x, y, offset)
    }

    pub(crate) fn set_details(&self, details: Rc<DetailsController>) {
        self.imp().surface.details.replace(details);
    }

    pub(crate) fn event_cards(&self) -> Vec<(i64, EventCard)> {
        self.imp().surface.event_cards()
    }

    pub fn set_week(&self, week: Rc<WeekSnapshot>) {
        self.clear_drop_preview();
        let surface = &self.imp().surface;
        let keys = week
            .days
            .iter()
            .flat_map(|day| &day.timed)
            .map(|segment| CardKey::Timed(segment.id()))
            .collect();
        surface.reconcile(self.upcast_ref(), week.locale.clone(), keys);
        for (card, segment) in surface
            .cards
            .borrow()
            .iter()
            .zip(week.days.iter().flat_map(|day| &day.timed))
        {
            card.button.set_timed(segment, week.locale.as_ref());
        }
        self.imp().week.replace(Some(week));
        self.queue_resize();
    }
}

/// Clips a destination to one day, retaining the cards' minimum visible height.
fn preview_bounds(date: NaiveDate, start: NaiveDateTime, end: NaiveDateTime) -> Option<(f32, f32)> {
    let midnight = date.and_hms_opt(0, 0, 0)?;
    let next = midnight.checked_add_signed(Duration::days(1))?;
    if start >= next || end < midnight || (end == midnight && start < midnight) {
        return None;
    }
    let y = (start.max(midnight) - midnight).num_seconds() as f32 / 60.0;
    let bottom = (end.min(next) - midnight).num_seconds() as f32 / 60.0;
    let y = y.min(1422.0);
    Some((y + 1.0, (bottom - y).max(18.0).min(1440.0 - y) - 2.0))
}

/// Snaps the occurrence start to quarter hours, retaining the grabbed segment's offset.
fn drop_start(start: NaiveDate, width: i32, x: f64, y: f64, offset: i64) -> Option<NaiveDateTime> {
    if width <= GUTTER
        || !x.is_finite()
        || !y.is_finite()
        || x < f64::from(GUTTER)
        || x >= f64::from(width)
        || !(0.0..1440.0).contains(&y)
    {
        return None;
    }
    let day = ((x - f64::from(GUTTER)) * 7.0 / f64::from(width - GUTTER)).floor() as i64;
    let minute = ((y - offset as f64) / 15.0).round() as i64 * 15;
    start
        .and_hms_opt(0, 0, 0)?
        .checked_add_signed(Duration::days(day))?
        .checked_add_signed(Duration::minutes(minute))
}

#[cfg(test)]
mod tests {
    use super::{GUTTER, drop_start, preview_bounds};

    #[test]
    fn preview_clips_multiday_events_and_excludes_an_exclusive_midnight_end() {
        let start = "2026-10-13T23:00:00".parse().unwrap();
        let end = "2026-10-15T00:00:00".parse().unwrap();
        for (date, expected) in [
            ("2026-10-12", None),
            ("2026-10-13", Some((1381.0, 58.0))),
            ("2026-10-14", Some((1.0, 1438.0))),
            ("2026-10-15", None),
        ] {
            assert_eq!(preview_bounds(date.parse().unwrap(), start, end), expected);
        }
    }

    #[test]
    fn preview_keeps_short_events_visible_at_the_end_of_the_day() {
        let date = "2026-10-14".parse().unwrap();
        let start = "2026-10-14T23:59:00".parse().unwrap();
        assert_eq!(preview_bounds(date, start, start), Some((1423.0, 16.0)));
    }

    #[test]
    fn drop_coordinates_snap_start_and_preserve_the_grabbed_segment_offset() {
        let start = "2026-10-12".parse().unwrap();
        let width = GUTTER + 700;
        let x = f64::from(GUTTER + 250);
        assert_eq!(
            drop_start(start, width, x, 613.0, 10).unwrap().to_string(),
            "2026-10-14 10:00:00"
        );
        assert_eq!(
            drop_start(start, width, x, 613.0, 1450)
                .unwrap()
                .to_string(),
            "2026-10-13 10:00:00",
            "a continuation drags the whole occurrence"
        );
        assert!(drop_start(start, width, 10.0, 600.0, 0).is_none());
        assert!(drop_start(start, width, f64::from(width), 600.0, 0).is_none());
        assert!(drop_start(start, width, x, -1.0, 0).is_none());
        assert!(drop_start(start, width, x, 1440.0, 0).is_none());
        assert!(drop_start(start, width, f64::NAN, 600.0, 0).is_none());
    }
}
