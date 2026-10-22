// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, rc::Rc};

use chrono::{NaiveDate, NaiveDateTime, Utc};
use eventix_view::WeekSnapshot;
use gtk::{glib, graphene, prelude::*, subclass::prelude::*};

use crate::actions::items::move_event::{self, DraggedEvent, MoveTarget};
use crate::view::geometry::{GUTTER, MIN_WIDTH, day_x, rect};
use crate::{
    style,
    view::{
        card::EventCard,
        creation_hint::{CreationHint, snapshot_hint},
        popover::DetailsController,
        surface::{CardKey, Surface},
    },
};

const ROW: i32 = 30;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct AllDayStrip {
        pub(super) surface: Rc<Surface>,
        pub week: RefCell<Option<Rc<WeekSnapshot>>>,
        pub(super) creation_hint: Rc<CreationHint>,
        pub(super) drop_preview: RefCell<Option<(NaiveDate, NaiveDate)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AllDayStrip {
        const NAME: &'static str = "EventixAllDayStrip";
        type Type = super::AllDayStrip;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for AllDayStrip {
        fn constructed(&self) {
            self.parent_constructed();
            self.surface.construct(self.obj().upcast_ref());
            move_event::install_target(&*self.obj());
            self.creation_hint
                .install(&*self.obj(), |strip, x, y| strip.date_at(x, y).is_some());
        }

        fn dispose(&self) {
            self.surface.dispose(self.obj().upcast_ref());
        }
    }

    impl WidgetImpl for AllDayStrip {
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
                self.week
                    .borrow()
                    .as_ref()
                    .and_then(|week| week.all_day.iter().map(|span| span.row + 1).max())
                    .unwrap_or(1) as i32
                    * ROW
            };
            (size, size, -1, -1)
        }

        fn size_allocate(&self, width: i32, _: i32, _: i32) {
            if let Some(week) = self.week.borrow().as_ref() {
                for (card, span) in self.surface.cards.borrow().iter().zip(&week.all_day) {
                    let left = day_x(width, span.start_day as f64).round() as i32;
                    let right = day_x(width, span.end_day as f64).round() as i32;
                    card.button.size_allocate(
                        &gtk::Allocation::new(
                            left + 2,
                            span.row as i32 * ROW + 1,
                            (right - left - 4).max(1),
                            ROW - 4,
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
            let height = obj.height() as f32;
            let line = style::color(&*obj, "ev_grid_line");
            if let Some(week) = self.week.borrow().as_ref() {
                let today = Utc::now()
                    .with_timezone(week.locale.timezone())
                    .date_naive();
                for (i, day) in week.days.iter().enumerate() {
                    if day.date == today {
                        let x = day_x(obj.width(), i as f64) as f32;
                        let right = day_x(obj.width(), (i + 1) as f64) as f32;
                        rect(
                            snapshot,
                            &style::color(&*obj, "ev_today_background"),
                            x,
                            0.0,
                            right - x,
                            height,
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
            self.surface.snapshot_cards(obj.upcast_ref(), snapshot);
            obj.snapshot_drop_preview(snapshot);
            if self.drop_preview.borrow().is_none() {
                obj.snapshot_creation_hint(snapshot);
            }
        }
    }
}

glib::wrapper! {
    pub struct AllDayStrip(ObjectSubclass<imp::AllDayStrip>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AllDayStrip {
    fn snapshot_creation_hint(&self, snapshot: &gtk::Snapshot) {
        let Some((x, y)) = self.imp().creation_hint.position() else {
            return;
        };
        let Some(col) = drop_day(self.width(), self.height(), x, y) else {
            return;
        };
        if self.imp().week.borrow().is_none() {
            return;
        }
        let left = day_x(self.width(), col as f64) as f32;
        let right = day_x(self.width(), (col + 1) as f64) as f32;
        snapshot_hint(
            self,
            snapshot,
            graphene::Rect::new(left, 0.0, right - left, self.height() as f32),
        );
    }

    pub(crate) fn date_at(&self, x: f64, y: f64) -> Option<NaiveDate> {
        let col = drop_day(self.width(), self.height(), x, y)?;
        Some(self.imp().week.borrow().as_ref()?.days.get(col)?.date)
    }

    fn snapshot_drop_preview(&self, snapshot: &gtk::Snapshot) {
        let preview = self.imp().drop_preview.borrow();
        let week = self.imp().week.borrow();
        let (Some((start, end)), Some(week)) = (preview.as_ref(), week.as_ref()) else {
            return;
        };
        let border = style::color(self, "ev_drop_border");
        for (col, day) in week.days.iter().enumerate() {
            if day.date < *start || day.date > *end {
                continue;
            }
            let x = day_x(self.width(), col as f64) as f32 + 2.0;
            let w = (day_x(self.width(), (col + 1) as f64) as f32 - x - 2.0).max(1.0);
            let h = (self.height() as f32 - 4.0).max(1.0);
            rect(
                snapshot,
                &style::color(self, "ev_drop_background"),
                x,
                2.0,
                w,
                h,
            );
            rect(snapshot, &border, x, 2.0, w, 2.0);
            rect(snapshot, &border, x, h, w, 2.0);
            rect(snapshot, &border, x, 2.0, 2.0, h);
            rect(snapshot, &border, x + w - 2.0, 2.0, 2.0, h);
            // Keep the label legible over existing all-day cards.
            let label_y = self.height() as f32 - ROW as f32;
            rect(
                snapshot,
                &border,
                x + 2.0,
                label_y,
                (w - 4.0).max(1.0),
                26.0,
            );
            let layout = self.create_pango_layout(Some(week.locale.translate("All day")));
            layout.set_width(((w - 12.0).max(0.0) * gtk::pango::SCALE as f32) as i32);
            layout.set_ellipsize(gtk::pango::EllipsizeMode::End);
            snapshot.save();
            snapshot.translate(&graphene::Point::new(x + 6.0, label_y + 3.0));
            snapshot.append_layout(&layout, &style::color(self, "ev_drop_text"));
            snapshot.restore();
        }
    }

    pub(crate) fn set_details(&self, details: Rc<DetailsController>) {
        self.imp().surface.details.replace(details);
    }

    pub(crate) fn event_cards(&self) -> Vec<(i64, EventCard)> {
        self.imp().surface.event_cards()
    }

    pub fn set_week(&self, week: Rc<WeekSnapshot>) {
        self.imp().creation_hint.clear(self);
        self.clear_drop_preview();
        let surface = &self.imp().surface;
        let keys = week
            .all_day
            .iter()
            .map(|span| CardKey::Span(span.event.id.clone()))
            .collect();
        surface.reconcile(self.upcast_ref(), week.locale.clone(), keys);
        for (card, span) in surface.cards.borrow().iter().zip(&week.all_day) {
            card.button.set_span(span, week.locale.as_ref());
            card.button.enable_date_drag();
        }
        self.imp().week.replace(Some(week));
        self.queue_resize();
    }
}

impl MoveTarget for AllDayStrip {
    fn move_start(&self, payload: &DraggedEvent, x: f64, y: f64) -> Option<NaiveDateTime> {
        if !payload.all_day {
            return None;
        }
        let col = drop_day(self.width(), self.height(), x, y)?;
        let week = self.imp().week.borrow();
        Some(payload.on_date(week.as_ref()?.days.get(col)?.date))
    }

    fn show_move_preview(&self, payload: &DraggedEvent, x: f64, y: f64) -> bool {
        let Some(start) = self.move_start(payload, x, y) else {
            return false;
        };
        let week = self.imp().week.borrow();
        let Some((start, end)) = week
            .as_ref()
            .and_then(|week| payload.preview_range(start, week.locale.timezone()))
        else {
            return false;
        };
        let preview = (start.date_naive(), end.date_naive());
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

fn drop_day(width: i32, height: i32, x: f64, y: f64) -> Option<usize> {
    if width <= GUTTER
        || !x.is_finite()
        || !y.is_finite()
        || x < f64::from(GUTTER)
        || x >= f64::from(width)
        || y < 0.0
        || y >= f64::from(height)
    {
        return None;
    }
    Some(((x - f64::from(GUTTER)) * 7.0 / f64::from(width - GUTTER)).floor() as usize)
}

#[cfg(test)]
mod tests {
    use super::{GUTTER, drop_day};

    #[test]
    fn drop_days_cover_all_rows_and_exclude_the_gutter_and_outside_coordinates() {
        let width = GUTTER + 700;
        assert_eq!(drop_day(width, 60, f64::from(GUTTER), 0.0), Some(0));
        assert_eq!(drop_day(width, 60, f64::from(GUTTER + 250), 45.0), Some(2));
        assert_eq!(drop_day(width, 60, f64::from(width - 1), 59.0), Some(6));
        for (x, y) in [
            (f64::from(GUTTER - 1), 0.0),
            (f64::from(width), 0.0),
            (100.0, -1.0),
            (100.0, 60.0),
            (f64::NAN, 0.0),
            (100.0, f64::NAN),
        ] {
            assert_eq!(drop_day(width, 60, x, y), None);
        }
        assert_eq!(drop_day(GUTTER, 60, 100.0, 0.0), None);
    }
}
