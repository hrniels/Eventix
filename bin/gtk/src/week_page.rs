// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    thread,
};

use chrono::{Datelike, NaiveDate, Utc};
use eventix_locale::{DateFlags, Locale};
use eventix_view::WeekSnapshot;
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};

use crate::{
    style,
    timeline::{GUTTER, Timeline},
};

/// Loads an existing local profile, reporting missing settings instead of an empty calendar.
fn load_local_state(xdg: Arc<xdg::BaseDirectories>) -> anyhow::Result<eventix_state::State> {
    let state = eventix_state::State::new(xdg.clone())?;
    if xdg.find_config_file("settings.toml").is_none() {
        let expected = xdg
            .get_config_file("settings.toml")
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "settings.toml".into());
        anyhow::bail!(
            formatx::formatx!(state.locale().translate(
            "No calendar configuration found at {}. Check XDG_CONFIG_HOME and --data-prefix."
        ), expected)
            .unwrap()
        );
    }
    Ok(state)
}

struct Request {
    generation: u64,
    date: Option<NaiveDate>,
    reload: bool,
}

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/hrniels/Eventix/gtk/week_page.ui")]
    pub struct WeekPage {
        #[template_child]
        pub title: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub status: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub headers: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub gutter: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub monday: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub tuesday: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub wednesday: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub thursday: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub friday: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub saturday: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub sunday: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub scroll: gtk::TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub timed: gtk::TemplateChild<Timeline>,
        #[template_child]
        pub all_day: gtk::TemplateChild<Timeline>,
        #[template_child]
        pub all_day_label: gtk::TemplateChild<gtk::Label>,
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
        pub(super) requests: RefCell<Option<async_channel::Sender<Request>>>,
        pub generation: Cell<u64>,
        pub date: Cell<Option<NaiveDate>>,
        pub displayed: Cell<Option<NaiveDate>>,
        pub updating_picker: Cell<bool>,
        pub locale: RefCell<Option<Arc<dyn Locale + Send + Sync>>>,
        pub timer: RefCell<Option<glib::SourceId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WeekPage {
        const NAME: &'static str = "EventixWeekPage";
        type Type = super::WeekPage;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            Timeline::ensure_type();
            klass.bind_template();
        }
        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for WeekPage {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.translate_labels();
            self.gutter.set_size_request(GUTTER, -1);
            self.headers.set_margin_end(
                self.scroll
                    .vscrollbar()
                    .measure(gtk::Orientation::Horizontal, -1)
                    .1,
            );
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
    impl WidgetImpl for WeekPage {}
    impl BoxImpl for WeekPage {}
}

glib::wrapper! {
    pub struct WeekPage(ObjectSubclass<imp::WeekPage>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl WeekPage {
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
        imp.previous
            .set_tooltip_text(Some(locale.translate("Previous week")));
        imp.next
            .set_tooltip_text(Some(locale.translate("Next week")));
        imp.today.set_label(locale.translate("Today"));
        imp.reload
            .set_tooltip_text(Some(locale.translate("Reload local calendars")));
        imp.picker
            .set_tooltip_text(Some(locale.translate("Choose date")));
        imp.all_day_label.set_text(locale.translate("All day"));
    }

    pub fn request(&self, date: Option<NaiveDate>, reload: bool) {
        let imp = self.imp();
        imp.date.set(date);
        let generation = imp.generation.get() + 1;
        imp.generation.set(generation);
        imp.status
            .set_text(self.locale().translate("Loading calendar…"));
        if let Some(requests) = imp.requests.borrow().as_ref() {
            let _ = requests.try_send(Request {
                generation,
                date,
                reload,
            });
        }
    }

    pub fn navigate(&self, days: i64) {
        let date = self.imp().date.get().unwrap_or_else(|| {
            Utc::now()
                .with_timezone(self.locale().timezone())
                .date_naive()
        });
        if let Some(date) = date.checked_add_signed(chrono::Duration::days(days)) {
            self.request(Some(date), false);
        }
    }

    pub fn reload(&self) {
        self.request(self.imp().date.get(), true);
    }

    fn show_week(&self, week: WeekSnapshot) {
        let imp = self.imp();
        if let Some((_, styles)) = imp.styles.borrow().as_ref() {
            styles.load_from_data(&style::calendar_css(&week));
        }
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
            label.set_text(&locale.fmt_weekdate(&day.date, DateFlags::NoToday));
        }
        let empty = week
            .days
            .iter()
            .all(|d| d.timed.is_empty() && d.all_day.is_empty());
        imp.status.set_text(locale.translate(if empty {
            "No events this week"
        } else {
            "Read-only · Local calendars"
        }));
        imp.date.set(Some(week.start));
        if let Ok(date) = glib::DateTime::from_local(
            week.start.year(),
            week.start.month() as i32,
            week.start.day() as i32,
            12,
            0,
            0.0,
        ) {
            imp.updating_picker.set(true);
            imp.calendar.select_day(&date);
            imp.updating_picker.set(false);
        }
        let reset_scroll = imp.displayed.replace(Some(week.start)) != Some(week.start);
        let week = Rc::new(week);
        imp.timed.set_week(week.clone());
        imp.all_day.set_week(week);
        self.update_today();
        if reset_scroll {
            let scroll = imp.scroll.downgrade();
            imp.timed.add_tick_callback(move |_, _| {
                if let Some(scroll) = scroll.upgrade() {
                    scroll.vadjustment().set_value(480.0);
                }
                glib::ControlFlow::Break
            });
        }
    }

    fn day_labels(&self) -> [&gtk::Label; 7] {
        let imp = self.imp();
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

    fn update_today(&self) {
        let imp = self.imp();
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
        imp.timed.queue_draw();
        imp.all_day.queue_draw();
    }

    pub fn start(&self, prefix: String, date: Option<NaiveDate>) {
        let (requests, rx) = async_channel::unbounded::<Request>();
        let (results, responses) = async_channel::unbounded();
        self.imp().requests.replace(Some(requests));
        let provider = gtk::CssProvider::new();
        let display = self.display();
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        self.imp().styles.replace(Some((display, provider)));
        // State loading and recurrence expansion can block; the worker never touches GTK objects.
        thread::spawn(move || {
            let xdg = Arc::new(xdg::BaseDirectories::with_prefix(prefix));
            let mut state = None;
            while let Ok(mut request) = rx.recv_blocking() {
                while let Ok(newer) = rx.try_recv() {
                    let reload = request.reload || newer.reload;
                    request = newer;
                    request.reload = reload;
                }
                let result = (|| -> anyhow::Result<(WeekSnapshot, String)> {
                    if state.is_none() || request.reload {
                        state = Some(load_local_state(xdg.clone())?);
                    }
                    let state = state.as_ref().expect("state was successfully loaded");
                    let date = request
                        .date
                        .unwrap_or_else(|| Utc::now().with_timezone(state.timezone()).date_naive());
                    let week = WeekSnapshot::load(state, date)?;
                    let config = xdg
                        .find_config_file("settings.toml")
                        .or_else(|| xdg.get_config_file("settings.toml"))
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    let data = xdg
                        .get_data_home()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    let source = format!(
                        "{}\n{}",
                        formatx::formatx!(
                            state.locale().translate("Calendar configuration: {}"),
                            config
                        )
                        .unwrap(),
                        formatx::formatx!(state.locale().translate("Calendar data: {}"), data)
                            .unwrap()
                    );
                    Ok((week, source))
                })()
                .map_err(|e| format!("{e:#}"));
                if results.send_blocking((request.generation, result)).is_err() {
                    break;
                }
            }
        });
        let weak = self.downgrade();
        let timer = glib::timeout_add_seconds_local(60, move || {
            let Some(page) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            page.update_today();
            glib::ControlFlow::Continue
        });
        self.imp().timer.replace(Some(timer));
        self.request(date, false);
        let weak = self.downgrade();
        glib::MainContext::default().spawn_local(async move {
            while let Ok((generation, result)) = responses.recv().await {
                let Some(page) = weak.upgrade() else { break };
                if page.imp().requests.borrow().is_none() {
                    break;
                }
                if generation != page.imp().generation.get() {
                    continue;
                }
                match result {
                    Ok((week, source)) => {
                        page.show_week(week);
                        page.imp().status.set_tooltip_text(Some(&source));
                    }
                    Err(error) => page.imp().status.set_text(&format!(
                        "{}\n{error}",
                        page.locale().translate("Could not load local calendars")
                    )),
                }
            }
        });
    }

    pub fn stop(&self) {
        if let Some(requests) = self.imp().requests.take() {
            requests.close();
        }
        if let Some(timer) = self.imp().timer.take() {
            timer.remove();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc};

    use super::load_local_state;

    #[test]
    fn missing_profile_reports_expected_path_but_explicit_empty_profile_loads() {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(tmp.path(), tmp.path()));
        let error = load_local_state(xdg.clone())
            .err()
            .expect("missing profile must fail");
        assert!(
            error
                .to_string()
                .contains(&tmp.path().join("settings.toml").display().to_string())
        );
        let settings = eventix_state::Settings::new(tmp.path().join("settings.toml"));
        settings.write_to_file().unwrap();
        let before = fs::read(tmp.path().join("settings.toml")).unwrap();
        let state = load_local_state(xdg).unwrap();
        assert!(state.store().directories().is_empty());
        assert_eq!(fs::read(tmp.path().join("settings.toml")).unwrap(), before);
    }
}
