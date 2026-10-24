// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, rc::Rc, sync::Arc};

use chrono::NaiveDate;
use eventix_ical::objects::CalCompType;
use eventix_locale::Locale;
use eventix_state::actions::{
    Action,
    items::{ChangeOccurrence, ChangeTask, RespondItem},
};
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};

use self::{content::PageContent, editor::PageEditor, session::PageSession};
use super::{
    card::EventCard, monthly::grid::MonthGrid, popover::DetailsController, sidebar::Sidebar,
    weekly::content::WeekContent,
};
use crate::model::session::View;

mod content;
mod create;
mod editor;
mod session;

mod imp {
    use super::*;

    resource_template! { "view/page.ui",
    pub struct CalendarPage {
        #[template_child]
        pub stack: gtk::TemplateChild<gtk::Stack>,
        #[template_child]
        pub header: gtk::TemplateChild<gtk::CenterBox>,
        #[template_child]
        pub sidebar: gtk::TemplateChild<Sidebar>,
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

        pub(super) editor: PageEditor,
        pub(super) content: PageContent,
        pub(super) session: PageSession,
        pub calendar_css: RefCell<String>,
        pub header_group: RefCell<Option<gtk::SizeGroup>>,
        pub styles: RefCell<Option<(gdk::Display, gtk::CssProvider)>>,
        pub locale: RefCell<Option<Arc<dyn Locale + Send + Sync>>>,
        pub(super) details: Rc<DetailsController>,
    }
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
            obj.setup_details();
            obj.align_headers();
            self.content.connect_controls(&obj);
            obj.connect_event_creation();
        }

        fn dispose(&self) {
            self.obj().stop();

            if let Some((display, provider)) = self.styles.take() {
                gtk::style_context_remove_provider_for_display(&display, &provider);
            }

            self.dispose_template();
        }
    }

    impl WidgetImpl for CalendarPage {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            self.details.allocate(self.obj().upcast_ref());
        }
    }

    impl BoxImpl for CalendarPage {}
}

glib::wrapper! {
    pub struct CalendarPage(ObjectSubclass<imp::CalendarPage>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl CalendarPage {
    fn setup_details(&self) {
        let imp = self.imp();
        imp.weekly.imp().timed.set_details(imp.details.clone());
        imp.weekly.imp().all_day.set_details(imp.details.clone());
        imp.monthly.set_details(imp.details.clone());
        imp.sidebar.set_details(imp.details.clone());

        let weak = self.downgrade();
        imp.details.connect_edit(move |item, scope| {
            if let Some(page) = weak.upgrade() {
                page.imp().editor.edit_item(&page, item, scope);
            }
        });

        let weak = self.downgrade();
        imp.details.connect_delete(move |item, recurrent, summary| {
            if let Some(page) = weak.upgrade() {
                page.imp()
                    .session
                    .confirm_delete(&page, item, recurrent, &summary);
            }
        });

        let weak = self.downgrade();
        imp.details.connect_respond(move |item, response| {
            if let Some(page) = weak.upgrade()
                && page.execute_command(Box::new(RespondItem { item, response }))
            {
                page.dismiss_popovers();
            }
        });

        let weak = self.downgrade();
        imp.details.connect_occurrence(move |item, action| {
            if let Some(page) = weak.upgrade()
                && page.execute_command(Box::new(ChangeOccurrence { item, action }))
            {
                page.dismiss_popovers();
            }
        });

        let weak = self.downgrade();
        imp.details.connect_dismissed(move || {
            if let Some(page) = weak.upgrade() {
                page.imp().editor.discard_editor();
            }
        });

        let weak = self.downgrade();
        imp.details.connect_task(move |item, action| {
            if let Some(page) = weak.upgrade()
                && page.execute_command(Box::new(ChangeTask { item, action }))
            {
                page.dismiss_popovers();
            }
        });

        let weak = self.downgrade();
        imp.details.connect_reminders(move |command| {
            if let Some(page) = weak.upgrade() {
                page.imp().session.save_reminders(&page, command);
            }
        });
    }

    /// Aligns the calendar header with the sidebar panel titles.
    fn align_headers(&self) {
        let imp = self.imp();
        let group = gtk::SizeGroup::new(gtk::SizeGroupMode::Vertical);
        group.add_widget(&*imp.header);
        for title in imp.sidebar.panel_titles() {
            group.add_widget(&title);
        }

        imp.header_group.replace(Some(group));
    }

    fn locale(&self) -> Arc<dyn Locale + Send + Sync> {
        self.imp()
            .locale
            .borrow()
            .clone()
            .unwrap_or_else(eventix_locale::default)
    }

    fn show_error(&self, message: &str, error: &str) {
        self.imp()
            .status
            .set_text(&format!("{}\n{error}", self.locale().translate(message)));
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

    fn install_styles(&self) {
        let provider = gtk::CssProvider::new();
        let display = self.display();
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        self.imp().styles.replace(Some((display, provider)));
    }

    pub fn editor_open(&self) -> bool {
        self.imp().editor.editor_open()
    }

    /// Handles editor shortcuts before either the window or a native popup consumes them.
    pub fn handle_editor_key(
        &self,
        key: gdk::Key,
        modifiers: gdk::ModifierType,
    ) -> Option<glib::Propagation> {
        self.imp().editor.handle_editor_key(self, key, modifiers)
    }

    pub fn new_event(&self) {
        self.imp().editor.new_item(self, CalCompType::Event);
    }

    pub fn new_task(&self) {
        self.imp().editor.new_item(self, CalCompType::Todo);
    }

    pub fn close_editor(&self) {
        self.imp().editor.finish_editor(self, None);
    }

    pub fn save_editor(&self) {
        self.imp().editor.save_editor(self);
    }

    pub fn show_content(&self, date: Option<NaiveDate>) {
        self.imp().content.show_content(self, date);
    }

    pub fn navigate(&self, direction: i32) {
        self.imp().content.navigate(self, direction);
    }

    pub fn focus_event(&self, current: Option<&EventCard>, forward: bool) {
        self.imp().content.focus_event(self, current, forward);
    }

    pub fn switch_to_week(&self) {
        self.imp().content.switch_view(self, View::Week);
    }

    pub fn switch_to_month(&self) {
        self.imp().content.switch_view(self, View::Month);
    }

    pub fn show_week_at(&self, date: NaiveDate) {
        self.imp().content.show_week_at(self, date);
    }

    pub fn dismiss_popovers(&self) {
        self.imp().details.close();
    }

    pub fn set_popup_owner_visible(&self, visible: bool) {
        self.imp().details.set_owner_visible(visible);
    }

    pub(crate) fn can_edit(&self) -> bool {
        self.imp().session.can_edit(self)
    }

    pub(crate) fn execute_command(&self, command: Box<dyn Action>) -> bool {
        self.imp().session.execute_command(self, command)
    }

    pub fn reload(&self) {
        self.imp().session.reload(self);
    }

    pub fn start(&self, prefix: String, date: Option<NaiveDate>) {
        self.imp().session.start(self, prefix, date);
    }

    pub fn stop(&self) {
        self.imp().session.stop(self);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::rc::Rc;

    use chrono::{Datelike, Duration, TimeZone};
    use eventix_state::actions::items::UpdateItem;
    use eventix_view::editor::EditorOptions;
    use eventix_view::{
        Day, MonthDay, MonthEvent, MonthSnapshot, TimedEvent, WeekSnapshot, layout::Columns,
    };
    use gtk::{prelude::*, subclass::prelude::*};

    use super::content::event_focus_index;
    use super::{CalendarPage, DetailsController};
    use crate::model::session::{ContentResponse, SavedItem, Snapshot, View};
    use crate::view::editor::ItemEditor;

    impl CalendarPage {
        fn show_week(&self, week: WeekSnapshot) {
            self.imp().content.show_week(self, week);
        }

        fn show_month(&self, month: MonthSnapshot) {
            self.imp().content.show_month(self, month);
        }

        fn receive_content(&self, response: ContentResponse) {
            self.imp().content.receive_content(self, response);
        }

        pub(in crate::view) fn test_existing_editor(
            &self,
            options: EditorOptions,
            edit: UpdateItem,
        ) -> Rc<ItemEditor> {
            self.imp().locale.replace(Some(options.locale.clone()));
            self.imp()
                .editor
                .present_existing_editor(self, 1, options, edit);
            self.imp().editor.current.borrow().clone().unwrap()
        }

        pub(in crate::view) fn test_new_editor(&self, options: EditorOptions) -> Rc<ItemEditor> {
            self.imp()
                .editor
                .present_editor(self, ItemEditor::new(1, options));
            self.imp().editor.current.borrow().clone().unwrap()
        }

        pub(in crate::view) fn test_details(&self) -> Rc<DetailsController> {
            self.imp().details.clone()
        }

        pub(in crate::view) fn test_saved_editor(&self, item: SavedItem) {
            self.imp().editor.finish_editor(self, Some(&item));
        }
    }

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

        let find_popup = |_: &gtk::Widget| page.imp().details.current();
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
        let header_bounds = imp.header.compute_bounds(&page).unwrap();
        let title_bounds = imp.sidebar.panel_titles()[0].compute_bounds(&page).unwrap();
        assert!(
            (header_bounds.y() - title_bounds.y()).abs() < 0.5
                && (header_bounds.height() - title_bounds.height()).abs() <= 1.0,
            "the calendar header line aligns with the events panel title"
        );
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
        let popup = page.imp().details.current().unwrap();
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
        page.show_content(Some(selected));
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
        let old_generation = imp.content.generation.get();
        imp.month_view.set_active(true);
        assert!(imp.content.view.get() == View::Month);
        assert_eq!(
            imp.content.date.get(),
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
        page.receive_content(ContentResponse {
            generation: old_generation,
            result: Ok(Snapshot::Week(WeekSnapshot {
                start: week.start,
                days: week.days.clone(),
                all_day: week.all_day.clone(),
                locale: week.locale.clone(),
            })),
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
        let find_popup = || page.imp().details.current();
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
        let span_popup = page.imp().details.current().unwrap();
        assert!(span_popup.is_mapped(), "spanning cards open shared details");
        imp.week_view.set_active(true);
        drain();
        assert!(find_popup().is_none(), "switching closes the monthly popup");
        assert!(
            span_popup.parent().is_none(),
            "switching closes span details"
        );
        assert!(imp.content.view.get() == View::Week);
        assert_eq!(imp.content.date.get(), Some(selected));
        page.navigate(1);
        assert_eq!(imp.content.date.get(), Some(selected + Duration::days(7)));
        imp.month_view.set_active(true);
        page.navigate(1);
        assert_eq!(
            imp.content.date.get(),
            eventix_view::shift_month(selected + Duration::days(7), 1)
        );
        let monday = make_month().start;
        imp.monthly.imp().weeks.borrow()[0].emit_clicked();
        assert!(imp.content.view.get() == View::Week);
        assert_eq!(
            imp.content.date.get(),
            Some(monday),
            "week number opens its Monday"
        );
        page.show_content(None);
        assert_eq!(
            imp.content.date.get(),
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
            week.locale.translate("Local calendars"),
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
