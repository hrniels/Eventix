// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use chrono::{Datelike, NaiveDate, Utc};
use eventix_locale::{DateFlags, Locale};
use eventix_view::{MonthSnapshot, WeekSnapshot, shift_month};
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};

use super::{
    card::EventCard,
    loader::{CalendarLoader, Request, Response, Snapshot, View},
    monthly::grid::MonthGrid,
    popover::DetailsController,
    sidebar::Sidebar,
    weekly::content::WeekContent,
};
use crate::style;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/hrniels/Eventix/gtk/view/page.ui")]
    pub struct CalendarPage {
        #[template_child]
        pub stack: gtk::TemplateChild<gtk::Stack>,
        #[template_child]
        pub sidebar: gtk::TemplateChild<Sidebar>,
        pub calendar_css: RefCell<String>,
        pub loading: Cell<bool>,
        #[template_child]
        pub weekly_content: gtk::TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub monthly_content: gtk::TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub monthly: gtk::TemplateChild<MonthGrid>,
        #[template_child]
        pub week_view: gtk::TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub month_view: gtk::TemplateChild<gtk::ToggleButton>,
        pub(super) view: Cell<View>,
        #[template_child]
        pub title: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub status: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub weekly: gtk::TemplateChild<WeekContent>,
        #[template_child]
        pub previous: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub next: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub today: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub reload: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub picker: gtk::TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub calendar: gtk::TemplateChild<gtk::Calendar>,
        pub styles: RefCell<Option<(gdk::Display, gtk::CssProvider)>>,
        pub(super) loader: RefCell<Option<CalendarLoader>>,
        pub generation: Cell<u64>,
        pub date: Cell<Option<NaiveDate>>,
        pub displayed: Cell<Option<NaiveDate>>,
        pub updating_picker: Cell<bool>,
        pub locale: RefCell<Option<Arc<dyn Locale + Send + Sync>>>,
        pub timer: RefCell<Option<glib::SourceId>>,
        pub(super) details: Rc<DetailsController>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CalendarPage {
        const NAME: &'static str = "EventixCalendarPage";
        type Type = super::CalendarPage;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            WeekContent::ensure_type();
            MonthGrid::ensure_type();
            Sidebar::ensure_type();
            klass.bind_template();
        }
        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for CalendarPage {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            self.weekly.imp().timed.set_details(self.details.clone());
            self.weekly.imp().all_day.set_details(self.details.clone());
            self.monthly.set_details(self.details.clone());
            self.sidebar.set_details(self.details.clone());
            let weak = obj.downgrade();
            self.week_view.connect_toggled(move |button| {
                if button.is_active()
                    && let Some(page) = weak.upgrade()
                {
                    page.switch_view(View::Week);
                }
            });
            let weak = obj.downgrade();
            self.month_view.connect_toggled(move |button| {
                if button.is_active()
                    && let Some(page) = weak.upgrade()
                {
                    page.switch_view(View::Month);
                }
            });
            obj.translate_labels();
            let weak = obj.downgrade();
            self.calendar.connect_day_selected(move |calendar| {
                let Some(page) = weak.upgrade() else { return };
                if page.imp().updating_picker.get() || !page.imp().picker.is_active() {
                    return;
                }
                let date = calendar.date();
                if let Some(date) = NaiveDate::from_ymd_opt(
                    date.year(),
                    date.month() as u32,
                    date.day_of_month() as u32,
                ) {
                    page.imp().picker.popdown();
                    page.request(Some(date), false);
                }
            });
        }

        fn dispose(&self) {
            self.obj().stop();
            if let Some((display, provider)) = self.styles.take() {
                gtk::style_context_remove_provider_for_display(&display, &provider);
            }
            self.dispose_template();
        }
    }
    impl WidgetImpl for CalendarPage {}
    impl BoxImpl for CalendarPage {}
}

glib::wrapper! {
    pub struct CalendarPage(ObjectSubclass<imp::CalendarPage>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl CalendarPage {
    fn locale(&self) -> Arc<dyn Locale + Send + Sync> {
        self.imp()
            .locale
            .borrow()
            .clone()
            .unwrap_or_else(eventix_locale::default)
    }

    fn translate_labels(&self) {
        let imp = self.imp();
        let locale = self.locale();
        let monthly = imp.view.get() == View::Month;
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

    pub fn request(&self, date: Option<NaiveDate>, reload: bool) {
        self.dismiss_popovers();
        let imp = self.imp();
        imp.date.set(date);
        let generation = imp.generation.get() + 1;
        imp.generation.set(generation);
        imp.loading.set(true);
        imp.status
            .set_text(self.locale().translate("Loading calendar…"));
        if let Some(loader) = imp.loader.borrow().as_ref() {
            loader.request(Request {
                generation,
                date,
                reload,
                view: imp.view.get(),
                sidebar_only: false,
            });
        }
    }

    pub fn navigate(&self, direction: i32) {
        let date = self.imp().date.get().unwrap_or_else(|| {
            Utc::now()
                .with_timezone(self.locale().timezone())
                .date_naive()
        });
        let next = match self.imp().view.get() {
            View::Week => date.checked_add_signed(chrono::Duration::days(i64::from(direction) * 7)),
            View::Month => shift_month(date, direction),
        };
        if let Some(date) = next {
            self.request(Some(date), false);
        }
    }

    pub fn focus_event(&self, current: Option<&EventCard>, forward: bool) {
        let imp = self.imp();
        let cards = if imp.view.get() == View::Month {
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
                if let Some(window) = self.root().and_downcast::<gtk::Window>() {
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

    pub fn reload(&self) {
        self.request(self.imp().date.get(), true);
    }

    fn switch_view(&self, view: View) {
        let imp = self.imp();
        if imp.view.replace(view) == view {
            return;
        }
        self.dismiss_popovers();
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
        imp.displayed.set(None);
        self.translate_labels();
        self.request(imp.date.get(), false);
    }

    pub fn show_week_at(&self, date: NaiveDate) {
        self.imp().date.set(Some(date));
        if self.imp().view.get() == View::Week {
            self.request(Some(date), false);
        } else {
            self.switch_view(View::Week);
        }
    }

    fn update_picker(&self) {
        let imp = self.imp();
        let date = imp.date.get().unwrap_or_else(|| {
            Utc::now()
                .with_timezone(self.locale().timezone())
                .date_naive()
        });
        imp.date.set(Some(date));
        if let Ok(date) = glib::DateTime::from_local(
            date.year(),
            date.month() as i32,
            date.day() as i32,
            12,
            0,
            0.0,
        ) {
            imp.updating_picker.set(true);
            imp.calendar.select_day(&date);
            imp.updating_picker.set(false);
        }
    }

    fn show_month(&self, month: MonthSnapshot) {
        let imp = self.imp();
        imp.calendar_css.replace(style::events_css(
            month
                .days
                .iter()
                .flat_map(|d| d.events.iter().map(|e| &e.event))
                .chain(month.spans.iter().flatten().map(|span| &span.event)),
        ));
        self.update_styles();
        imp.locale.replace(Some(month.locale.clone()));
        self.translate_labels();
        if let Some(window) = self.root().and_downcast::<gtk::Window>() {
            window.set_title(Some(&format!(
                "Eventix — {}",
                month.locale.translate("Monthly View")
            )));
        }
        imp.title.set_text(&format!(
            "{} {} · {}",
            month
                .locale
                .translate(&month.month.format("%B").to_string()),
            month.month.year(),
            month.locale.timezone(),
        ));
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
            "Read-only · Local calendars"
        }));
        self.update_picker();
        if imp.displayed.replace(Some(month.month)) != Some(month.month) {
            imp.monthly_content.vadjustment().set_value(0.0);
        }
        imp.monthly.set_month(Rc::new(month));
    }

    fn show_week(&self, week: WeekSnapshot) {
        let imp = self.imp();
        imp.calendar_css.replace(style::calendar_css(&week));
        self.update_styles();
        imp.locale.replace(Some(week.locale.clone()));
        self.translate_labels();
        let locale = &week.locale;
        if let Some(window) = self.root().and_downcast::<gtk::Window>() {
            window.set_title(Some(&format!(
                "Eventix — {}",
                locale.translate("Weekly View")
            )));
        }
        imp.title.set_text(&format!(
            "{} — {} · {} · {}",
            locale.fmt_weekdate(&week.start, DateFlags::NoToday),
            locale.fmt_weekdate(&week.days[6].date, DateFlags::NoToday),
            formatx::formatx!(locale.translate("Week {}"), week.start.iso_week().week()).unwrap(),
            locale.timezone()
        ));
        for (label, day) in self.day_labels().into_iter().zip(&week.days) {
            label.set_text(&locale.fmt_weekdate(&day.date, DateFlags::Short | DateFlags::NoToday));
        }
        let empty = week.all_day.is_empty() && week.days.iter().all(|d| d.timed.is_empty());
        imp.status.set_text(locale.translate(if empty {
            "No events this week"
        } else {
            "Read-only · Local calendars"
        }));
        self.update_picker();
        let reset_scroll = imp.displayed.replace(Some(week.start)) != Some(week.start);
        let week = Rc::new(week);
        imp.weekly.imp().timed.set_week(week.clone());
        imp.weekly.imp().all_day.set_week(week);
        self.update_today();
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

    fn day_labels(&self) -> [&gtk::Label; 7] {
        let imp = self.imp().weekly.imp();
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

    pub fn dismiss_popovers(&self) {
        self.imp().details.close();
    }

    fn update_today(&self) {
        let imp = self.imp();
        if imp.view.get() == View::Month {
            imp.monthly.update_today();
            return;
        }
        let Some(start) = imp.displayed.get() else {
            return;
        };
        let today = Utc::now()
            .with_timezone(self.locale().timezone())
            .date_naive();
        for (i, label) in self.day_labels().into_iter().enumerate() {
            if (today - start).num_days() == i as i64 {
                label.add_css_class("ev_today");
            } else {
                label.remove_css_class("ev_today");
            }
        }
        imp.weekly.imp().timed.queue_draw();
        imp.weekly.imp().all_day.queue_draw();
    }

    pub fn start(&self, prefix: String, date: Option<NaiveDate>) {
        let (loader, responses) = CalendarLoader::start(prefix);
        self.imp().loader.replace(Some(loader));
        let provider = gtk::CssProvider::new();
        let display = self.display();
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        self.imp().styles.replace(Some((display, provider)));
        let weak = self.downgrade();
        let timer = glib::timeout_add_seconds_local(60, move || {
            let Some(page) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            page.update_today();
            let imp = page.imp();
            if !imp.loading.get()
                && let Some(loader) = imp.loader.borrow().as_ref()
            {
                loader.request(Request {
                    generation: imp.generation.get(),
                    date: imp.date.get(),
                    reload: false,
                    view: imp.view.get(),
                    sidebar_only: true,
                });
            }
            glib::ControlFlow::Continue
        });
        self.imp().timer.replace(Some(timer));
        self.request(date, false);
        let weak = self.downgrade();
        glib::MainContext::default().spawn_local(async move {
            while let Ok(response) = responses.recv().await {
                let Some(page) = weak.upgrade() else { break };
                if page.imp().loader.borrow().is_none() {
                    break;
                }
                page.receive_response(response);
            }
        });
    }

    fn receive_response(&self, response: Response) {
        if response.generation != self.imp().generation.get() {
            return;
        }
        match response.result {
            Ok((snapshot, source)) => {
                if let Some(sidebar) = response.sidebar {
                    self.imp().sidebar.set_snapshot(sidebar);
                    self.update_styles();
                }
                match snapshot {
                    Snapshot::Week(week) => self.show_week(week),
                    Snapshot::Month(month) => self.show_month(month),
                    Snapshot::Sidebar => {}
                }
                self.imp().loading.set(false);
                self.imp().status.set_tooltip_text(Some(&source));
            }
            Err(error) => {
                self.imp().loading.set(false);
                self.imp().status.set_text(&format!(
                    "{}\n{error}",
                    self.locale().translate("Could not load local calendars"),
                ));
            }
        }
    }

    fn update_styles(&self) {
        let imp = self.imp();
        if let Some((_, styles)) = imp.styles.borrow().as_ref() {
            styles.load_from_data(&format!(
                "{}{}",
                imp.calendar_css.borrow(),
                imp.sidebar.colors_css()
            ));
        }
    }

    pub fn stop(&self) {
        self.imp().loader.take();
        self.dismiss_popovers();
        if let Some(timer) = self.imp().timer.take() {
            timer.remove();
        }
    }
}

/// Selects an event when entering the calendar, keeping navigation inside its bounds.
fn event_focus_index(count: usize, current: Option<usize>, forward: bool) -> Option<usize> {
    match current {
        None if forward => (count > 0).then_some(0),
        None => count.checked_sub(1),
        Some(index) if forward => index.checked_add(1).filter(|next| *next < count),
        Some(index) => index.checked_sub(1),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use chrono::{Datelike, Duration, TimeZone};
    use eventix_view::{
        Day, MonthDay, MonthEvent, MonthSnapshot, TimedEvent, WeekSnapshot, layout::Columns,
    };
    use gtk::{prelude::*, subclass::prelude::*};

    use super::event_focus_index;
    use super::{CalendarPage, Response, Snapshot, View};

    /// Checks real widget allocation, focus retention and cross-surface popup coordination.
    /// Called by the display-backed details test so GTK stays on one test thread.
    pub(crate) fn assert_span_layout_and_refresh(mut week: WeekSnapshot) {
        let mut span = week.all_day[0].clone();
        span.end_day = 3;
        let mut adjacent = span.clone();
        adjacent.event.id.uid = "adjacent".into();
        adjacent.start_day = 3;
        adjacent.end_day = 5;
        let mut overlapping = span.clone();
        overlapping.event.id.uid = "overlapping".into();
        overlapping.start_day = 1;
        overlapping.row = 1;
        week.all_day = vec![span, adjacent, overlapping];
        week.days = (0..7)
            .map(|day| Day {
                date: week.start + chrono::Duration::days(day),
                timed: vec![],
                dst: vec![],
            })
            .collect();
        week.days[0].timed.push(TimedEvent {
            event: week.all_day[0].event.clone(),
            date: week.start,
            minute_start: 540,
            minute_end: 600,
            columns: Columns {
                count: 1,
                offset: 0,
                span: 1,
            },
        });
        // Change placement and order while keeping the occurrence identity unchanged.
        let mut refreshed = WeekSnapshot {
            start: week.start,
            days: week.days.clone(),
            locale: week.locale.clone(),
            all_day: week.all_day.clone(),
        };
        refreshed.all_day[0].end_day = 2;
        refreshed.all_day.swap(0, 1);
        let page: CalendarPage = gtk::glib::Object::new();
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(800)
            .child(&page)
            .build();
        page.show_week(week);
        window.present();
        let drain = || {
            for _ in 0..20 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        drain();
        let imp = page.imp().weekly.imp();
        let cards = imp.all_day.event_cards();
        assert_eq!(cards.len(), 3, "one card is created per span");
        let card = &cards[0].1;
        let allocation = card.allocation();
        let right = crate::view::geometry::day_x(imp.all_day.width(), 3.0).round() as i32;
        assert_eq!(allocation.x() + allocation.width(), right - 2);
        assert_eq!(allocation.y(), cards[1].1.allocation().y());
        assert!(cards[1].1.allocation().x() >= allocation.x() + allocation.width());
        assert!(cards[2].1.allocation().y() > allocation.y());
        assert!(card.grab_focus());
        page.show_week(refreshed);
        drain();
        assert_eq!(
            &imp.all_day.event_cards()[1].1,
            card,
            "the live spanning card is reused"
        );
        assert!(card.has_focus(), "repacking retains keyboard focus");
        assert!(
            card.width() < allocation.width(),
            "updated spans change their pixel width"
        );

        let find_popup = |widget: &gtk::Widget| {
            let mut child = widget.first_child();
            while let Some(widget) = child {
                if let Some(popup) = widget.downcast_ref::<crate::view::details::EventDetails>() {
                    return Some(popup.clone());
                }
                child = widget.next_sibling();
            }
            None
        };
        card.emit_clicked();
        drain();
        let strip_popup = find_popup(imp.all_day.upcast_ref()).unwrap();
        assert!(strip_popup.is_mapped());
        imp.timed.event_cards()[0].1.emit_clicked();
        drain();
        assert!(
            !strip_popup.is_visible(),
            "opening timed details closes strip details"
        );
        assert!(strip_popup.parent().is_none());
        let timed_popup = find_popup(imp.timed.upcast_ref()).unwrap();
        assert!(timed_popup.is_mapped());
        card.emit_clicked();
        drain();
        assert!(
            !timed_popup.is_visible(),
            "opening strip details closes timed details"
        );
        assert!(timed_popup.parent().is_none());
        page.dismiss_popovers();
        drain();
        assert!(find_popup(imp.all_day.upcast_ref()).is_none());
        window.close();
        drain();
    }

    /// Checks sidebar geometry, independent scrolling, retained rows and task popups.
    pub(crate) fn assert_sidebar_layout_and_details(mut week: WeekSnapshot) {
        use eventix_view::sidebar::{SidebarGroup, SidebarRow, SidebarSnapshot};

        while week.days.len() < 7 {
            week.days.push(Day {
                date: week.start + Duration::days(week.days.len() as i64),
                timed: vec![],
                dst: vec![],
            });
        }

        let event = week.all_day[0].event.clone();
        let row = SidebarRow {
            id: event.id.clone(),
            summary: "Sidebar entry with a long summary".into(),
            calendar: event.calendar.clone(),
            background: "#803366".into(),
            foreground: "#ffffff".into(),
            details: event.details.clone(),
            start: Some(event.start),
            due: Some(event.end),
            full_day: false,
            time: "09:00".into(),
            task: false,
            overdue: false,
            due_today: false,
            cancelled: false,
            recurrent: true,
            exception: true,
            alarm: true,
            response: None,
        };
        let make_snapshot = || {
            let events = (0..40)
                .map(|index| {
                    let mut row = row.clone();
                    row.id.uid = format!("sidebar-event-{index}");
                    row.full_day = index == 0;
                    row
                })
                .collect();
            let tasks = (0..40)
                .map(|index| {
                    let mut row = row.clone();
                    row.id.uid = format!("sidebar-task-{index}");
                    row.task = true;
                    row.overdue = true;
                    row.due = None;
                    row.start = None;
                    row.time.clear();
                    row
                })
                .collect();
            SidebarSnapshot {
                events: vec![SidebarGroup {
                    date: Some(week.start),
                    rows: events,
                }],
                tasks: vec![SidebarGroup {
                    date: None,
                    rows: tasks,
                }],
                locale: week.locale.clone(),
            }
        };
        let page: CalendarPage = gtk::glib::Object::new();
        let window = gtk::Window::builder()
            .default_width(1200)
            .default_height(700)
            .child(&page)
            .build();
        page.show_week(WeekSnapshot {
            start: week.start,
            days: week.days.clone(),
            all_day: week.all_day.clone(),
            locale: week.locale.clone(),
        });
        let imp = page.imp();
        imp.sidebar.set_snapshot(make_snapshot());
        let provider = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(
            &WidgetExt::display(&window),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        imp.styles
            .replace(Some((WidgetExt::display(&window), provider.clone())));
        page.update_styles();
        window.present();
        let drain = || {
            for _ in 0..20 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        drain();
        let events = imp.sidebar.first_child().unwrap();
        let tasks = imp.sidebar.last_child().unwrap();
        assert!((events.height() - tasks.height()).abs() <= 1);
        let stack_bounds = imp.stack.compute_bounds(&page).unwrap();
        let sidebar_bounds = imp.sidebar.compute_bounds(&page).unwrap();
        assert!(sidebar_bounds.x() >= stack_bounds.x() + stack_bounds.width());
        assert!((stack_bounds.width() / sidebar_bounds.width() - 4.0).abs() < 0.5);
        let event_scroll = events
            .downcast_ref::<gtk::Frame>()
            .unwrap()
            .child()
            .unwrap()
            .last_child()
            .unwrap()
            .downcast::<gtk::ScrolledWindow>()
            .unwrap();
        let task_scroll = tasks
            .downcast_ref::<gtk::Frame>()
            .unwrap()
            .child()
            .unwrap()
            .last_child()
            .unwrap()
            .downcast::<gtk::ScrolledWindow>()
            .unwrap();
        assert!(event_scroll.vadjustment().upper() > event_scroll.vadjustment().page_size());
        assert!(task_scroll.vadjustment().upper() > task_scroll.vadjustment().page_size());
        event_scroll.vadjustment().set_value(60.0);
        assert_eq!(task_scroll.vadjustment().value(), 0.0);
        let list = task_scroll
            .child()
            .unwrap()
            .downcast::<gtk::Viewport>()
            .unwrap()
            .child()
            .unwrap();
        let button = list
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        assert!(button.grab_focus());
        imp.sidebar.set_snapshot(make_snapshot());
        drain();
        assert!(button.has_focus(), "refresh retains the live focused row");
        assert_eq!(event_scroll.vadjustment().value(), 60.0);
        button.emit_clicked();
        drain();
        let popup = button
            .last_child()
            .unwrap()
            .downcast::<crate::view::details::EventDetails>()
            .unwrap();
        assert!(popup.is_mapped(), "undated tasks open shared details");
        page.dismiss_popovers();
        drain();
        assert!(popup.parent().is_none());
        assert!(
            provider
                .to_string()
                .contains(&crate::style::calendar_class("#803366", "#ffffff")),
            "sidebar-only calendar colors are installed"
        );
        window.set_default_size(1000, 450);
        imp.month_view.set_active(true);
        let (start, end) = eventix_view::month_range(week.start).unwrap();
        page.show_month(MonthSnapshot {
            month: week.start.with_day(1).unwrap(),
            start,
            days: (0..(end - start).num_days())
                .map(|offset| MonthDay {
                    date: start + Duration::days(offset),
                    current_month: true,
                    show_month: false,
                    events: vec![],
                })
                .collect(),
            spans: vec![vec![]; ((end - start).num_days() / 7) as usize],
            locale: week.locale.clone(),
        });
        drain();
        assert!(
            (events.height() - tasks.height()).abs() <= 1,
            "equal split survives resizing and switching"
        );
        assert!(button.parent().is_some(), "switching keeps sidebar rows");
        if let Some(directory) = std::env::var_os("EVENTIX_DETAILS_SCREENSHOTS") {
            let paintable = gtk::WidgetPaintable::new(Some(&page));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(&snapshot, page.width().into(), page.height().into());
            window
                .renderer()
                .unwrap()
                .render_texture(snapshot.to_node().unwrap(), None)
                .save_to_png(std::path::PathBuf::from(directory).join("sidebar.png"))
                .unwrap();
        }
        imp.sidebar.set_snapshot(SidebarSnapshot {
            events: vec![],
            tasks: vec![],
            locale: week.locale,
        });
        drain();
        assert!(
            list.first_child().is_none(),
            "empty panels have no placeholder rows"
        );
        window.close();
        drain();
    }

    /// Checks monthly scrolling, refresh identity, popups, switching and view-aware navigation.
    /// Runs on the existing display-backed test's GTK thread.
    pub(crate) fn assert_month_layout_and_navigation(mut week: WeekSnapshot) {
        while week.days.len() < 7 {
            week.days.push(Day {
                date: week.start + Duration::days(week.days.len() as i64),
                timed: vec![],
                dst: vec![],
            });
        }
        let selected = week.start + Duration::days(2);
        let make_month = || {
            let (start, end) = eventix_view::month_range(selected).unwrap();
            let mut days: Vec<_> = (0..(end - start).num_days())
                .map(|offset| {
                    let date = start + Duration::days(offset);
                    MonthDay {
                        date,
                        current_month: date.month() == selected.month(),
                        show_month: date.day() == 1
                            || date.succ_opt().unwrap().month() != date.month(),
                        events: vec![],
                    }
                })
                .collect();
            let index = (week.start - start).num_days() as usize;
            for number in 0..18 {
                let mut event = week.all_day[0].event.clone();
                event.id.uid = format!("monthly-{number}");
                event.summary = format!("Monthly entry {number}");
                if number == 0 {
                    event.all_day = false;
                    event.start = week
                        .locale
                        .timezone()
                        .with_ymd_and_hms(
                            week.start.year(),
                            week.start.month(),
                            week.start.day(),
                            12,
                            0,
                            0,
                        )
                        .unwrap()
                        - Duration::days(1);
                    event.end = event.start + Duration::days(2);
                }
                days[index].events.push(MonthEvent {
                    event,
                    date: week.start,
                    full_day: true,
                });
            }
            // The same occurrence on a different day must have a distinct live widget.
            let mut repeated = days[index].events[0].clone();
            repeated.date = days[index + 1].date;
            repeated.full_day = false;
            days[index + 1].events.push(repeated);
            MonthSnapshot {
                month: selected.with_day(1).unwrap(),
                start,
                spans: vec![vec![]; days.len() / 7],
                days,
                locale: week.locale.clone(),
            }
        };
        let page: CalendarPage = gtk::glib::Object::new();
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(450)
            .child(&page)
            .build();
        let group = gtk::gio::SimpleActionGroup::new();
        let action = gtk::gio::SimpleAction::new("show-week", Some(gtk::glib::VariantTy::STRING));
        let weak = page.downgrade();
        action.connect_activate(move |_, date| {
            let page = weak.upgrade().unwrap();
            page.show_week_at(
                chrono::NaiveDate::parse_from_str(date.unwrap().str().unwrap(), "%Y-%m-%d")
                    .unwrap(),
            );
        });
        group.add_action(&action);
        window.insert_action_group("win", Some(&group));
        page.request(Some(selected), false);
        page.show_week(WeekSnapshot {
            start: week.start,
            days: week.days.clone(),
            all_day: week.all_day.clone(),
            locale: week.locale.clone(),
        });
        window.present();
        let drain = || {
            for _ in 0..20 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        drain();
        let imp = page.imp();
        let old_generation = imp.generation.get();
        imp.month_view.set_active(true);
        assert!(imp.view.get() == View::Month);
        assert_eq!(
            imp.date.get(),
            Some(selected),
            "switching retains the selected day"
        );
        page.show_month(make_month());
        drain();
        assert_eq!(
            imp.stack.visible_child(),
            Some(imp.monthly_content.clone().upcast())
        );
        assert_eq!(
            imp.previous.tooltip_text().as_deref(),
            Some(week.locale.translate("Previous month"))
        );
        assert_eq!(
            imp.monthly.imp().weekdays.borrow()[0].text(),
            week.locale.translate("Monday")
        );
        let title = imp.title.text();
        page.receive_response(Response {
            generation: old_generation,
            sidebar: None,
            result: Ok((
                Snapshot::Week(WeekSnapshot {
                    start: week.start,
                    days: week.days.clone(),
                    all_day: week.all_day.clone(),
                    locale: week.locale.clone(),
                }),
                "stale".into(),
            )),
        });
        assert_eq!(
            imp.title.text(),
            title,
            "stale weekly response cannot replace the month"
        );
        let cards = imp.monthly.event_cards();
        assert_eq!(cards.len(), 19);
        assert_ne!(cards[0].1, cards[18].1);
        assert_eq!(
            cards[0].1.tooltip_text().as_deref(),
            Some("Monthly entry 0")
        );
        assert_eq!(
            cards[18].1.tooltip_text().as_deref(),
            Some("← 12:00 Monthly entry 0")
        );
        for pair in cards[..18].windows(2) {
            let a = pair[0].1.allocation();
            let b = pair[1].1.allocation();
            assert!(
                b.y() >= a.y() + a.height(),
                "busy-day entries never overlap"
            );
        }
        let first = cards[0].1.allocation();
        let adjacent = cards[18].1.allocation();
        assert_eq!(adjacent.y(), first.y());
        let cells = imp.monthly.imp().scrollers.borrow();
        let index = (week.start - make_month().start).num_days() as usize;
        let busy_cell = cells[index / 7].clone();
        let neighbor = cells[index / 7 + 1].clone();
        let date_header = imp.monthly.imp().days.borrow()[index].allocation();
        let busy_bounds = busy_cell.compute_bounds(&*imp.monthly).unwrap();
        let neighbor_bounds = neighbor.compute_bounds(&*imp.monthly).unwrap();
        assert!(neighbor_bounds.y() >= busy_bounds.y() + busy_bounds.height());
        assert!(busy_cell.height() <= crate::view::monthly::grid::MAX_ROW);
        assert!(busy_cell.vadjustment().upper() > busy_cell.vadjustment().page_size());
        drop(cells);
        page.focus_event(Some(&cards[16].1), true);
        drain();
        assert!(cards[17].1.has_focus());
        assert!(
            busy_cell.vadjustment().value() > 0.0,
            "keyboard navigation scrolls within the busy week"
        );
        assert_eq!(
            neighbor.vadjustment().value(),
            0.0,
            "neighboring weeks scroll independently"
        );
        assert_eq!(
            imp.monthly.imp().days.borrow()[index].allocation(),
            date_header,
            "the date header remains outside the week scroller"
        );
        let mut refreshed = make_month();
        let index = (week.start - refreshed.start).num_days() as usize;
        refreshed.days[index].events.reverse();
        page.show_month(refreshed);
        drain();
        assert_eq!(imp.monthly.event_cards()[0].1, cards[17].1);
        assert!(
            cards[17].1.has_focus(),
            "refresh reuses the focused occurrence-day card"
        );
        let find_popup = || {
            let parent = cards[17].1.parent().unwrap();
            let mut child = parent.first_child();
            while let Some(widget) = child {
                if let Some(popup) = widget.downcast_ref::<crate::view::details::EventDetails>() {
                    return Some(popup.clone());
                }
                child = widget.next_sibling();
            }
            None
        };
        page.focus_event(None, true);
        drain();
        cards[17].1.emit_clicked();
        drain();
        assert!(
            find_popup().unwrap().is_mapped(),
            "monthly cards open shared details"
        );
        let mut spanning = make_month();
        let row = (week.start - spanning.start).num_days() as usize / 7;
        let mut span = week.all_day[0].clone();
        span.event.id.uid = "monthly-span".into();
        span.start_day = 0;
        span.end_day = 3;
        span.row = 0;
        span.continues_after = true;
        let mut overlap = span.clone();
        overlap.event.id.uid = "monthly-overlap".into();
        overlap.start_day = 1;
        overlap.end_day = 4;
        overlap.row = 1;
        let mut continuation = span.clone();
        continuation.end_day = 2;
        continuation.continues_before = true;
        continuation.continues_after = false;
        spanning.spans[row] = vec![span, overlap];
        spanning.spans[row + 1] = vec![continuation];
        page.show_month(spanning);
        drain();
        let span_cards: Vec<_> = imp
            .monthly
            .event_cards()
            .into_iter()
            .filter(|(_, card)| card.event().unwrap().id.uid.starts_with("monthly-s"))
            .map(|(_, card)| card)
            .collect();
        assert_eq!(span_cards.len(), 2, "one card per week-local span");
        assert_ne!(
            span_cards[0], span_cards[1],
            "continuations have separate widgets"
        );
        let first_span = &span_cards[0];
        let allocation = first_span.allocation();
        let parent = first_span.parent().unwrap();
        let right =
            crate::view::geometry::day_x(parent.width() + crate::view::geometry::GUTTER, 3.0)
                .round() as i32
                - crate::view::geometry::GUTTER;
        assert_eq!(allocation.x() + allocation.width(), right - 4);
        let overlap_card = imp
            .monthly
            .event_cards()
            .into_iter()
            .find(|(_, card)| card.event().unwrap().id.uid == "monthly-overlap")
            .unwrap()
            .1;
        assert!(overlap_card.allocation().y() >= allocation.y() + allocation.height());
        assert!(
            cards[0].1.parent().unwrap().allocation().y()
                >= overlap_card.allocation().y() + overlap_card.height(),
            "timed lists start below the shared span lanes"
        );
        assert_eq!(
            parent.width(),
            imp.monthly.width() - crate::view::geometry::GUTTER,
            "the scrollbar does not consume day-column width"
        );
        assert!(first_span.grab_focus());
        let mut refreshed = make_month();
        let mut spans = imp
            .monthly
            .imp()
            .month
            .borrow()
            .as_ref()
            .unwrap()
            .spans
            .clone();
        spans[row][0].end_day = 2;
        spans[row].swap(0, 1);
        refreshed.spans = spans;
        page.show_month(refreshed);
        drain();
        assert!(
            first_span.has_focus(),
            "refresh retains the focused span card"
        );
        assert!(first_span.width() < allocation.width());
        first_span.emit_clicked();
        drain();
        let mut child = parent.first_child();
        let span_popup = loop {
            let widget = child.expect("monthly span popup exists");
            if let Some(popup) = widget.downcast_ref::<crate::view::details::EventDetails>() {
                break popup.clone();
            }
            child = widget.next_sibling();
        };
        assert!(span_popup.is_mapped(), "spanning cards open shared details");
        imp.week_view.set_active(true);
        drain();
        assert!(find_popup().is_none(), "switching closes the monthly popup");
        assert!(
            span_popup.parent().is_none(),
            "switching closes span details"
        );
        assert!(imp.view.get() == View::Week);
        assert_eq!(imp.date.get(), Some(selected));
        page.navigate(1);
        assert_eq!(imp.date.get(), Some(selected + Duration::days(7)));
        imp.month_view.set_active(true);
        page.navigate(1);
        assert_eq!(
            imp.date.get(),
            eventix_view::shift_month(selected + Duration::days(7), 1)
        );
        let monday = make_month().start;
        imp.monthly.imp().weeks.borrow()[0].emit_clicked();
        assert!(imp.view.get() == View::Week);
        assert_eq!(imp.date.get(), Some(monday), "week number opens its Monday");
        page.request(None, false);
        assert_eq!(
            imp.date.get(),
            None,
            "Today delegates to the profile's timezone"
        );
        let mut all_day_only = make_month();
        for day in &mut all_day_only.days {
            day.events.clear();
        }
        let mut span = week.all_day[0].clone();
        span.start_day = 0;
        span.end_day = 3;
        all_day_only.spans[row] = vec![span.clone()];
        let styles = gtk::CssProvider::new();
        imp.styles
            .replace(Some((WidgetExt::display(&window), styles.clone())));
        page.show_month(all_day_only);
        drain();
        assert_eq!(
            imp.status.text(),
            week.locale.translate("Read-only · Local calendars"),
            "a month containing only spans is not empty"
        );
        assert!(
            styles.to_string().contains(&crate::style::calendar_class(
                &span.event.background,
                &span.event.foreground
            )),
            "calendar colors include events present only in span lanes"
        );
        window.close();
        drain();
    }

    #[test]
    fn arrows_enter_events_from_non_event_focus_and_stop_at_boundaries() {
        assert_eq!(event_focus_index(3, None, true), Some(0));
        assert_eq!(event_focus_index(3, None, false), Some(2));
        assert_eq!(event_focus_index(3, Some(0), true), Some(1));
        assert_eq!(event_focus_index(3, Some(2), false), Some(1));
        assert_eq!(event_focus_index(3, Some(2), true), None);
        assert_eq!(event_focus_index(3, Some(0), false), None);
        assert_eq!(event_focus_index(0, None, true), None);
        assert_eq!(event_focus_index(0, None, false), None);
    }
}
