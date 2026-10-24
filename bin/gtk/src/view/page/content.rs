// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::Cell, rc::Rc};

use chrono::{Datelike, NaiveDate, Utc};
use eventix_locale::DateFlags;
use eventix_view::{MonthSnapshot, WeekSnapshot, shift_month};
use gtk::{glib, prelude::*, subclass::prelude::*};

use super::CalendarPage;
use crate::model::session::{ContentRequest, ContentResponse, Snapshot, View};
use crate::style;
use crate::view::card::EventCard;

/// Owns the selected period and the state of calendar content requests.
#[derive(Default)]
pub(super) struct PageContent {
    pub(super) view: Cell<View>,
    pub(super) loading: Cell<bool>,
    /// Identifies the latest requested content selection, even while its result is pending.
    pub(super) generation: Cell<u64>,
    /// Records the last successfully displayed generation for subscription recovery on reload.
    pub(super) loaded_generation: Cell<Option<u64>>,
    pub(super) date: Cell<Option<NaiveDate>>,
    displayed: Cell<Option<NaiveDate>>,
    updating_picker: Cell<bool>,
}

impl PageContent {
    pub(super) fn connect_controls(&self, page: &CalendarPage) {
        let imp = page.imp();

        let weak = page.downgrade();
        imp.week_view.connect_toggled(move |button| {
            if button.is_active()
                && let Some(page) = weak.upgrade()
            {
                page.switch_to_week();
            }
        });

        let weak = page.downgrade();
        imp.month_view.connect_toggled(move |button| {
            if button.is_active()
                && let Some(page) = weak.upgrade()
            {
                page.switch_to_month();
            }
        });

        self.translate_labels(page);

        let weak = page.downgrade();
        imp.calendar.connect_day_selected(move |calendar| {
            let Some(page) = weak.upgrade() else { return };

            if page.imp().content.updating_picker.get() || !page.imp().picker.is_active() {
                return;
            }

            let date = calendar.date();
            if let Some(date) = NaiveDate::from_ymd_opt(
                date.year(),
                date.month() as u32,
                date.day_of_month() as u32,
            ) {
                page.imp().picker.popdown();
                page.show_content(Some(date));
            }
        });
    }

    fn translate_labels(&self, page: &CalendarPage) {
        let imp = page.imp();
        let locale = page.locale();
        let monthly = self.view.get() == View::Month;

        imp.previous
            .set_tooltip_text(Some(locale.translate(if monthly {
                "Previous month"
            } else {
                "Previous week"
            })));
        imp.next.set_tooltip_text(Some(locale.translate(if monthly {
            "Next month"
        } else {
            "Next week"
        })));

        imp.week_view.set_label(locale.translate("Week"));
        imp.week_view
            .set_tooltip_text(Some(locale.translate("Weekly View")));

        imp.month_view.set_label(locale.translate("Month"));
        imp.month_view
            .set_tooltip_text(Some(locale.translate("Monthly View")));

        imp.today.set_label(locale.translate("Today"));
        imp.reload
            .set_tooltip_text(Some(locale.translate("Reload local calendars")));
        imp.picker
            .set_tooltip_text(Some(locale.translate("Choose date")));
    }

    pub(super) fn show_content(&self, page: &CalendarPage, date: Option<NaiveDate>) {
        page.dismiss_popovers();

        let imp = page.imp();
        self.date.set(date);
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.loading.set(true);
        imp.status
            .set_text(page.locale().translate("Loading calendar…"));

        if let Some(session) = imp.session.current.borrow().as_ref() {
            session.show_content(ContentRequest {
                generation,
                date,
                view: self.view.get(),
            });
        }
    }

    pub(super) fn navigate(&self, page: &CalendarPage, direction: i32) {
        if page.editor_open() {
            return;
        }

        let date = self.date.get().unwrap_or_else(|| {
            Utc::now()
                .with_timezone(page.locale().timezone())
                .date_naive()
        });
        let next = match self.view.get() {
            View::Week => date.checked_add_signed(chrono::Duration::days(i64::from(direction) * 7)),
            View::Month => shift_month(date, direction),
        };
        if let Some(date) = next {
            self.show_content(page, Some(date));
        }
    }

    pub(super) fn focus_event(
        &self,
        page: &CalendarPage,
        current: Option<&EventCard>,
        forward: bool,
    ) {
        let imp = page.imp();
        let cards = if self.view.get() == View::Month {
            imp.monthly.event_cards()
        } else {
            let mut cards = imp.weekly.imp().timed.event_cards();
            cards.extend(imp.weekly.imp().all_day.event_cards());
            cards.sort_by_key(|(start, _)| *start);
            cards
        };

        let index = current.and_then(|current| cards.iter().position(|(_, card)| card == current));
        let next = event_focus_index(cards.len(), index, forward);
        if let Some(next) = next {
            let card = &cards[next].1;
            if card.grab_focus() {
                if let Some(window) = page.root().and_downcast::<gtk::Window>() {
                    window.set_focus_visible(true);
                }

                // Custom event-surface children can lie outside either scroll viewport.
                // Reveal the focused card in each enclosing scroller, inside out.
                let mut ancestor = card.parent();
                while let Some(widget) = ancestor {
                    if let Some(scroll) = widget.downcast_ref::<gtk::ScrolledWindow>()
                        && let Some(child) = scroll.child()
                    {
                        let content = child
                            .downcast_ref::<gtk::Viewport>()
                            .and_then(|viewport| viewport.child())
                            .unwrap_or(child);
                        if let Some(bounds) = card.compute_bounds(&content) {
                            scroll.vadjustment().clamp_page(
                                f64::from(bounds.y()),
                                f64::from(bounds.y() + bounds.height()),
                            );
                            scroll.hadjustment().clamp_page(
                                f64::from(bounds.x()),
                                f64::from(bounds.x() + bounds.width()),
                            );
                        }
                    }

                    ancestor = widget.parent();
                }
            }
        }
    }

    pub(super) fn switch_view(&self, page: &CalendarPage, view: View) {
        if page.editor_open() {
            return;
        }

        let imp = page.imp();
        if self.view.replace(view) == view {
            return;
        }

        page.dismiss_popovers();
        imp.picker.popdown();
        imp.week_view.set_active(view == View::Week);
        imp.month_view.set_active(view == View::Month);

        if view == View::Month {
            imp.stack.set_visible_child(&*imp.monthly_content);
        } else {
            imp.stack.set_visible_child(&*imp.weekly_content);
        }

        // Clear the old period's title until the new snapshot arrives.
        imp.title.set_text("");
        imp.title.set_tooltip_text(None);
        self.displayed.set(None);

        self.translate_labels(page);
        self.show_content(page, self.date.get());
    }

    pub(super) fn show_week_at(&self, page: &CalendarPage, date: NaiveDate) {
        self.date.set(Some(date));
        if self.view.get() == View::Week {
            self.show_content(page, Some(date));
        } else {
            self.switch_view(page, View::Week);
        }
    }

    fn update_picker(&self, page: &CalendarPage) {
        let imp = page.imp();
        let date = self.date.get().unwrap_or_else(|| {
            Utc::now()
                .with_timezone(page.locale().timezone())
                .date_naive()
        });
        self.date.set(Some(date));

        if let Ok(date) = glib::DateTime::from_local(
            date.year(),
            date.month() as i32,
            date.day() as i32,
            12,
            0,
            0.0,
        ) {
            self.updating_picker.set(true);
            imp.calendar.select_day(&date);
            self.updating_picker.set(false);
        }
    }

    pub(super) fn show_month(&self, page: &CalendarPage, month: MonthSnapshot) {
        let imp = page.imp();

        imp.calendar_css.replace(style::events_css(
            month
                .days
                .iter()
                .flat_map(|d| d.events.iter().map(|e| &e.event))
                .chain(month.spans.iter().flatten().map(|span| &span.event)),
        ));
        page.update_styles();

        imp.locale.replace(Some(month.locale.clone()));
        self.translate_labels(page);

        if let Some(window) = page.root().and_downcast::<gtk::Window>() {
            window.set_title(Some(&format!(
                "Eventix — {}",
                month.locale.translate("Monthly View")
            )));
        }

        let title = format!(
            "{} {}",
            month
                .locale
                .translate(&month.month.format("%B").to_string()),
            month.month.year(),
        );
        imp.title.set_text(&title);
        imp.title.set_tooltip_text(Some(&title));

        let empty = month
            .days
            .iter()
            .filter(|d| d.current_month)
            .all(|d| d.events.is_empty())
            && !month.spans.iter().enumerate().any(|(week, spans)| {
                spans.iter().any(|span| {
                    month.days[week * 7 + span.start_day..week * 7 + span.end_day]
                        .iter()
                        .any(|day| day.current_month)
                })
            });
        imp.status.set_text(month.locale.translate(if empty {
            "No events this month"
        } else {
            "Local calendars"
        }));

        self.update_picker(page);
        if self.displayed.replace(Some(month.month)) != Some(month.month) {
            imp.monthly_content.vadjustment().set_value(0.0);
        }

        imp.monthly.set_month(Rc::new(month));
    }

    pub(super) fn show_week(&self, page: &CalendarPage, week: WeekSnapshot) {
        let imp = page.imp();

        imp.calendar_css.replace(style::calendar_css(&week));
        page.update_styles();

        imp.locale.replace(Some(week.locale.clone()));
        self.translate_labels(page);

        let locale = &week.locale;
        if let Some(window) = page.root().and_downcast::<gtk::Window>() {
            window.set_title(Some(&format!(
                "Eventix — {}",
                locale.translate("Weekly View")
            )));
        }

        let title = formatx::formatx!(
            locale.translate("Week {}: {} — {}"),
            week.start.iso_week().week(),
            locale.fmt_weekdate(&week.start, DateFlags::NoToday),
            locale.fmt_weekdate(&week.days[6].date, DateFlags::NoToday)
        )
        .unwrap();
        imp.title.set_text(&title);
        imp.title.set_tooltip_text(Some(&title));

        for ((label, icon), day) in Self::day_labels(page)
            .into_iter()
            .zip(imp.weekly.imp().time_change_icons.borrow().iter())
            .zip(&week.days)
        {
            let text = locale.fmt_weekdate(&day.date, DateFlags::Short | DateFlags::NoToday);
            let time_change = !day.dst.is_empty();
            label.set_text(&text);
            icon.set_visible(time_change);
            icon.set_tooltip_text(time_change.then(|| locale.translate("Time change")));
            label.set_tooltip_text(time_change.then(|| locale.translate("Time change")));
        }

        let empty = week.all_day.is_empty() && week.days.iter().all(|d| d.timed.is_empty());
        imp.status.set_text(locale.translate(if empty {
            "No events this week"
        } else {
            "Local calendars"
        }));

        self.update_picker(page);

        let reset_scroll = self.displayed.replace(Some(week.start)) != Some(week.start);
        let week = Rc::new(week);
        imp.weekly.imp().timed.set_week(week.clone());
        imp.weekly.imp().all_day.set_week(week);
        self.update_today(page);

        if reset_scroll {
            let scroll = imp.weekly.imp().scroll.downgrade();
            imp.weekly.imp().timed.add_tick_callback(move |_, _| {
                if let Some(scroll) = scroll.upgrade() {
                    scroll.vadjustment().set_value(480.0);
                }

                glib::ControlFlow::Break
            });
        }
    }

    fn day_labels(page: &CalendarPage) -> [&gtk::Label; 7] {
        let imp = page.imp().weekly.imp();
        [
            &imp.monday,
            &imp.tuesday,
            &imp.wednesday,
            &imp.thursday,
            &imp.friday,
            &imp.saturday,
            &imp.sunday,
        ]
    }

    pub(super) fn update_today(&self, page: &CalendarPage) {
        let imp = page.imp();
        if self.view.get() == View::Month {
            imp.monthly.update_today();
            return;
        }

        let Some(start) = self.displayed.get() else {
            return;
        };

        let today = Utc::now()
            .with_timezone(page.locale().timezone())
            .date_naive();
        for (i, label) in Self::day_labels(page).into_iter().enumerate() {
            if (today - start).num_days() == i as i64 {
                label.add_css_class("ev_today");
            } else {
                label.remove_css_class("ev_today");
            }
        }

        imp.weekly.imp().timed.queue_draw();
        imp.weekly.imp().all_day.queue_draw();
    }

    pub(super) fn receive_content(&self, page: &CalendarPage, response: ContentResponse) {
        if response.generation != self.generation.get() {
            return;
        }

        match response.result {
            Ok(snapshot) => {
                match snapshot {
                    Snapshot::Week(week) => self.show_week(page, week),
                    Snapshot::Month(month) => self.show_month(page, month),
                }

                self.loaded_generation.set(Some(response.generation));
                self.loading.set(false);
            }
            Err(error) => {
                self.loading.set(false);
                page.show_error("Could not load local calendars", &error);
            }
        }
    }
}

/// Selects an event when entering the calendar, keeping navigation inside its bounds.
pub(super) fn event_focus_index(
    count: usize,
    current: Option<usize>,
    forward: bool,
) -> Option<usize> {
    match current {
        None if forward => (count > 0).then_some(0),
        None => count.checked_sub(1),
        Some(index) if forward => index.checked_add(1).filter(|next| *next < count),
        Some(index) => index.checked_sub(1),
    }
}
