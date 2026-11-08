// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::HashMap,
};

use eventix_locale::Locale;
use gtk::{glib, prelude::*, subclass::prelude::*};

use crate::model::activity::{Activity, Phase};
use crate::model::calendars::CalendarVisibility;

const HISTORY_LIMIT: u32 = 30;

struct HistoryEntry {
    text: String,
    icon: &'static str,
    kind: String,
}

mod imp {
    use super::*;

    resource_template! { "view/status_bar.ui",
    pub struct StatusBar {
        #[template_child]
        pub status: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub version: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub history_button: gtk::TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub history_count: gtk::TemplateChild<gtk::Label>,
        pub pending_count: Cell<usize>,
        #[template_child]
        pub history_popup: gtk::TemplateChild<gtk::Popover>,
        #[template_child]
        pub history_scroll: gtk::TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub history: gtk::TemplateChild<gtk::ListView>,
        pub history_entries: OnceCell<gtk::gio::ListStore>,
        pub calendar_names: RefCell<HashMap<String, String>>,
        #[template_child]
        pub sync_progress: gtk::TemplateChild<gtk::ProgressBar>,
        #[template_child]
        pub cancel_sync: gtk::TemplateChild<gtk::Button>,
        pub progress_timer: RefCell<Option<glib::SourceId>>,
    }
    }

    impl StatusBar {
        pub fn entries(&self) -> &gtk::gio::ListStore {
            self.history_entries
                .get_or_init(gtk::gio::ListStore::new::<glib::BoxedAnyObject>)
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for StatusBar {
        const NAME: &'static str = "EventixStatusBar";
        type Type = super::StatusBar;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for StatusBar {
        fn constructed(&self) {
            self.parent_constructed();
            self.version
                .set_text(concat!("Eventix v", env!("CARGO_PKG_VERSION")));
            self.history
                .set_model(Some(&gtk::NoSelection::new(Some(self.entries().clone()))));
            let factory = gtk::SignalListItemFactory::new();
            factory.connect_setup(|_, object| {
                let item = object.downcast_ref::<gtk::ListItem>().unwrap();
                let label = gtk::Label::builder()
                    .xalign(0.0)
                    .selectable(true)
                    .focusable(false)
                    .single_line_mode(true)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .hexpand(true)
                    .margin_top(5)
                    .margin_bottom(5)
                    .build();
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                row.append(&gtk::Image::new());
                row.append(&label);
                item.set_child(Some(&row));
            });
            factory.connect_bind(|_, object| {
                let item = object.downcast_ref::<gtk::ListItem>().unwrap();
                let object = item.item().and_downcast::<glib::BoxedAnyObject>().unwrap();
                let entry = object.borrow::<HistoryEntry>();
                let row = item.child().and_downcast::<gtk::Box>().unwrap();
                let icon = row.first_child().and_downcast::<gtk::Image>().unwrap();
                let label = row.last_child().and_downcast::<gtk::Label>().unwrap();
                icon.set_icon_name(Some(entry.icon));
                icon.set_tooltip_text(Some(&entry.kind));
                icon.update_property(&[gtk::accessible::Property::Label(&entry.kind)]);
                label.set_text(&entry.text);
                label.set_tooltip_text(Some(&entry.text));
            });
            self.history.set_factory(Some(&factory));
            self.history_scroll.connect_map(|scroll| {
                // Wait for the first mapped frame to allocate the list and its scroll range.
                let laid_out = Cell::new(false);
                scroll.add_tick_callback(move |scroll, _| {
                    if !laid_out.replace(true) {
                        return glib::ControlFlow::Continue;
                    }
                    let adjustment = scroll.vadjustment();
                    adjustment.set_value(adjustment.upper() - adjustment.page_size());
                    glib::ControlFlow::Break
                });
            });
        }

        fn dispose(&self) {
            self.obj().stop();
            self.dispose_template();
        }
    }

    impl WidgetImpl for StatusBar {}
    impl BoxImpl for StatusBar {}
}

glib::wrapper! {
    pub struct StatusBar(ObjectSubclass<imp::StatusBar>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl StatusBar {
    pub(crate) fn record(&self, activity: &Activity, locale: &dyn Locale) {
        let imp = self.imp();
        let names = imp.calendar_names.borrow();
        let text = activity.text(locale, |id| {
            names.get(id).cloned().unwrap_or_else(|| id.into())
        });
        imp.status.set_text(&activity.summary(locale));
        imp.status.set_tooltip_text(Some(&text));
        let time = activity.time().with_timezone(locale.timezone());
        let (icon, kind) = match activity.phase() {
            Phase::Started => ("dialog-information-symbolic", "Information"),
            Phase::Completed => ("emblem-ok-symbolic", "Completed"),
            Phase::Failed => ("dialog-error-symbolic", "Error"),
            Phase::Cancelled => ("process-stop-symbolic", "Cancelled"),
        };
        let entry = HistoryEntry {
            text: format!("{}  {text}", time.format("%H:%M:%S")),
            icon,
            kind: locale.translate(kind).into(),
        };
        let entries = imp.entries();
        if entries.n_items() >= HISTORY_LIMIT {
            entries.remove(0);
        }
        entries.append(&glib::BoxedAnyObject::new(entry));
        imp.history_button.set_sensitive(true);
    }

    pub(crate) fn set_calendars(&self, calendars: &[CalendarVisibility]) {
        self.imp().calendar_names.replace(
            calendars
                .iter()
                .map(|calendar| (calendar.id.clone(), calendar.name.clone()))
                .collect(),
        );
    }

    pub fn clear(&self, locale: &dyn Locale) {
        let imp = self.imp();
        imp.history_popup.popdown();
        imp.entries().remove_all();
        imp.calendar_names.borrow_mut().clear();
        imp.status.set_text("");
        imp.status.set_tooltip_text(None);
        self.set_pending_pushes(0, locale);
    }

    pub(crate) fn set_pending_pushes(&self, count: usize, locale: &dyn Locale) {
        let imp = self.imp();
        imp.pending_count.set(count);
        imp.history_count.set_text(&count.to_string());
        imp.history_button
            .set_sensitive(count > 0 || imp.entries().n_items() > 0);
        let outstanding =
            formatx::formatx!(locale.translate("{} outstanding pushes"), count).unwrap();
        let tooltip = format!("{}\n{}", locale.translate("Activity history"), outstanding);
        imp.history_button.set_tooltip_text(Some(&tooltip));
        imp.history_button
            .update_property(&[gtk::accessible::Property::Label(&tooltip)]);
    }

    pub fn set_locale(&self, locale: &dyn Locale) {
        self.set_pending_pushes(self.imp().pending_count.get(), locale);
        let cancel = &self.imp().cancel_sync;
        cancel.set_tooltip_text(Some(locale.translate("Cancel operation")));
        cancel.update_property(&[gtk::accessible::Property::Label(
            locale.translate("Cancel operation"),
        )]);
    }

    pub fn connect_cancel(&self, callback: impl Fn() + 'static) {
        self.imp().cancel_sync.connect_clicked(move |_| callback());
    }

    pub fn can_cancel(&self) -> bool {
        self.imp().cancel_sync.is_visible() && self.imp().cancel_sync.is_sensitive()
    }

    pub fn cancelling(&self) {
        self.imp().cancel_sync.set_sensitive(false);
    }

    pub fn start_operation(&self, locale: &dyn Locale, description: &str, determinate: bool) {
        self.stop();
        let imp = self.imp();
        imp.sync_progress.set_fraction(0.0);
        imp.sync_progress.set_visible(true);
        imp.cancel_sync.set_sensitive(true);
        imp.cancel_sync.set_visible(true);
        imp.sync_progress
            .set_tooltip_text(Some(locale.translate(description)));
        // Single-collection backends do not expose measurable intermediate progress.
        if !determinate {
            imp.sync_progress.pulse();
            let weak = self.downgrade();
            imp.progress_timer.replace(Some(glib::timeout_add_local(
                std::time::Duration::from_millis(100),
                move || {
                    let Some(status) = weak.upgrade() else {
                        return glib::ControlFlow::Break;
                    };
                    status.imp().sync_progress.pulse();
                    glib::ControlFlow::Continue
                },
            )));
        }
    }

    pub fn set_progress(&self, completed: usize, total: usize) {
        self.imp().sync_progress.set_fraction(if total == 0 {
            1.0
        } else {
            completed as f64 / total as f64
        });
    }

    pub fn finish_sync(&self, completed: bool, cancelled: bool) {
        self.stop();
        if completed && !cancelled {
            self.imp().sync_progress.set_fraction(1.0);
            self.imp().sync_progress.set_visible(true);
        }
        if !cancelled {
            let weak = self.downgrade();
            self.imp()
                .progress_timer
                .replace(Some(glib::timeout_add_local_once(
                    std::time::Duration::from_secs(1),
                    move || {
                        if let Some(status) = weak.upgrade() {
                            status.imp().progress_timer.take();
                            status.imp().sync_progress.set_visible(false);
                        }
                    },
                )));
        }
    }

    pub fn stop(&self) {
        if let Some(timer) = self.imp().progress_timer.take() {
            timer.remove();
        }
        self.imp().sync_progress.set_visible(false);
        self.imp().cancel_sync.set_visible(false);
    }

    #[cfg(test)]
    pub fn text(&self) -> glib::GString {
        self.imp().status.text()
    }
}

#[cfg(test)]
mod tests {
    use gtk::{glib, prelude::*, subclass::prelude::*};

    use super::StatusBar;
    use crate::model::activity::{Activity, Phase};

    fn first_entry_label(widget: &gtk::Widget) -> Option<gtk::Label> {
        if let Some(label) = widget.downcast_ref::<gtk::Label>()
            && label.is_selectable()
        {
            return Some(label.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if let Some(label) = first_entry_label(&widget) {
                return Some(label);
            }
            child = widget.next_sibling();
        }
        None
    }

    fn first_entry_icon(widget: &gtk::Widget) -> Option<gtk::Image> {
        if let Some(icon) = widget.downcast_ref::<gtk::Image>()
            && icon
                .icon_name()
                .as_deref()
                .is_some_and(|name| name.starts_with("dialog-"))
        {
            return Some(icon.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if let Some(icon) = first_entry_icon(&widget) {
                return Some(icon);
            }
            child = widget.next_sibling();
        }
        None
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn activity_button_shows_outstanding_push_count_and_keeps_history_popup() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let status: StatusBar = glib::Object::new();
        let locale = eventix_locale::default();
        status.set_locale(locale.as_ref());
        let imp = status.imp();
        assert_eq!(imp.history_count.text(), "0");
        assert!(!imp.history_button.is_sensitive());
        assert_eq!(
            imp.history_button.tooltip_text().as_deref(),
            Some("Activity history\n0 outstanding pushes")
        );
        status.set_pending_pushes(3, locale.as_ref());
        assert_eq!(imp.history_count.text(), "3");
        assert!(imp.history_button.is_sensitive());
        assert_eq!(
            imp.history_button.tooltip_text().as_deref(),
            Some("Activity history\n3 outstanding pushes")
        );
        assert_eq!(
            imp.sync_progress.next_sibling().as_ref(),
            Some(imp.cancel_sync.upcast_ref())
        );
        let row = imp
            .history_button
            .child()
            .and_downcast::<gtk::Box>()
            .unwrap();
        assert_eq!(
            row.first_child().as_ref(),
            Some(imp.history_count.upcast_ref())
        );
        let icon = row.last_child().and_downcast::<gtk::Image>().unwrap();
        assert_eq!(
            icon.icon_name().as_deref(),
            Some("document-open-recent-symbolic")
        );
        status.record(&Activity::new(1, "Items uploaded"), locale.as_ref());
        let window = gtk::Window::new();
        window.set_child(Some(&status));
        window.present();
        imp.history_button.popup();
        glib::MainContext::default()
            .block_on(glib::timeout_future(std::time::Duration::from_millis(100)));
        assert!(imp.history_popup.is_mapped());
        status.set_pending_pushes(0, locale.as_ref());
        assert_eq!(imp.history_count.text(), "0");
        assert!(imp.history_button.is_sensitive());
        assert!(imp.history_popup.is_mapped());
        assert_eq!(imp.entries().n_items(), 1);
        status.clear(locale.as_ref());
        assert!(!imp.history_button.is_sensitive());
        assert_eq!(
            imp.history_button.tooltip_text().as_deref(),
            Some("Activity history\n0 outstanding pushes")
        );
        window.close();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn settings_language_change_preserves_status_history() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let status: StatusBar = glib::Object::new();
        crate::style::install(&status.display());
        status.set_locale(eventix_locale::default().as_ref());
        let imp = status.imp();
        assert_eq!(
            status.first_child().as_ref(),
            Some(imp.history_button.upcast_ref())
        );
        let tmp = tempfile::tempdir().unwrap();
        let xdg = xdg::BaseDirectories::with_prefix(tmp.path().file_name().unwrap());
        let german = eventix_locale::new_with_timezone(
            &xdg,
            eventix_locale::LocaleType::German,
            chrono_tz::UTC,
        )
        .unwrap();
        status.set_pending_pushes(4, eventix_locale::default().as_ref());
        status.set_locale(german.as_ref());
        assert_eq!(imp.history_count.text(), "4");
        assert_eq!(
            imp.history_button.tooltip_text().as_deref(),
            Some("Aktivitätsverlauf\n4 ausstehende Uploads")
        );
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn history_opens_above_status_retains_entries_and_updates_while_open() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let status: StatusBar = glib::Object::new();
        let temporary = tempfile::tempdir().unwrap();
        let xdg = xdg::BaseDirectories::with_prefix(temporary.path().file_name().unwrap());
        let locale = eventix_locale::new_with_timezone(
            &xdg,
            eventix_locale::LocaleType::English,
            chrono_tz::UTC,
        )
        .unwrap();
        status.set_locale(locale.as_ref());
        let imp = status.imp();
        status.set_calendars(&[crate::model::calendars::CalendarVisibility {
            id: "calendar-id-123".into(),
            name: "Work".into(),
            background: "#ffffff".into(),
            enabled: true,
            sync_error: None,
        }]);
        assert!(!imp.history_button.is_sensitive());
        assert!(status.text().is_empty());
        let window = gtk::Window::builder()
            .default_width(800)
            .default_height(500)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        spacer.set_vexpand(true);
        content.append(&spacer);
        content.append(&status);
        window.set_child(Some(&content));
        window.present();
        let mut activity = Activity::new(7, "Item updated");
        activity.add_item(eventix_state::actions::items::ItemId {
            calendar: "calendar-id-123".into(),
            uid: "abc123".into(),
            recurrence: None,
        });
        status.record(&activity, locale.as_ref());
        assert!(imp.history_button.is_sensitive());
        imp.history_button.popup();
        let context = glib::MainContext::default();
        context.block_on(glib::timeout_future(std::time::Duration::from_millis(100)));
        assert!(imp.history_popup.is_mapped());
        let label = first_entry_label(imp.history.upcast_ref()).unwrap();
        assert!(label.text().contains("Calendar: Work, UID: abc123"));
        assert!(!label.text().contains("calendar-id-123"));
        assert_eq!(
            label.selection_bounds(),
            None,
            "opening history does not select text"
        );
        label.select_region(0, -1);
        assert!(
            label.selection_bounds().is_some(),
            "text can still be selected explicitly"
        );
        label.select_region(0, 0);
        assert_eq!(imp.history_popup.child().unwrap().width(), 600);
        assert_eq!(imp.history_popup.position(), gtk::PositionType::Top);
        assert_eq!(imp.history_popup.halign(), gtk::Align::Start);
        assert!(!imp.history_popup.has_arrow());
        let button_bounds = imp.history_button.compute_bounds(&window).unwrap();
        let text_bounds = imp.status.compute_bounds(&window).unwrap();
        assert!(button_bounds.x() + button_bounds.width() <= text_bounds.x());

        let activity = activity.finish(
            Phase::Failed,
            Some(format!("Could not save:\n\t{}", "detail ".repeat(100))),
        );
        status.record(&activity, locale.as_ref());
        context.block_on(glib::timeout_future(std::time::Duration::from_millis(100)));
        assert!(imp.history_popup.is_mapped());
        assert_eq!(imp.entries().n_items(), 2);
        let newest = imp
            .entries()
            .item(1)
            .and_downcast::<glib::BoxedAnyObject>()
            .unwrap();
        let newest = newest.borrow::<super::HistoryEntry>();
        assert_eq!(newest.icon, "dialog-error-symbolic");
        let newest = &newest.text;
        assert!(newest.contains("Item update failed — Calendar: Work, UID: abc123"));
        assert!(!newest.contains("#7"));
        assert!(!newest.contains("calendar-id-123"));
        assert!(newest.contains(": Could not save: detail"));
        assert!(!newest.contains(['\n', '\r', '\t']));
        let previous = imp
            .entries()
            .item(0)
            .and_downcast::<glib::BoxedAnyObject>()
            .unwrap();
        let previous = previous.borrow::<super::HistoryEntry>();
        assert!(previous.text.contains("Item update started"));
        assert_eq!(previous.icon, "dialog-information-symbolic");
        let label = first_entry_label(imp.history.upcast_ref()).unwrap();
        assert_eq!(label.selection_bounds(), None);
        let icon = first_entry_icon(imp.history.upcast_ref()).unwrap();
        assert_eq!(
            icon.icon_name().as_deref(),
            Some("dialog-information-symbolic")
        );
        assert!(!imp.status.wraps());
        assert!(imp.status.is_single_line_mode());
        assert_eq!(status.text(), "Item update failed");
        assert_eq!(
            imp.status.tooltip_text().unwrap(),
            activity.text(locale.as_ref(), |_| "Work".into())
        );
        for (phase, icon) in [
            (Phase::Completed, "emblem-ok-symbolic"),
            (Phase::Cancelled, "process-stop-symbolic"),
        ] {
            status.record(&activity.clone().finish(phase, None), locale.as_ref());
            let entry = imp
                .entries()
                .item(imp.entries().n_items() - 1)
                .and_downcast::<glib::BoxedAnyObject>()
                .unwrap();
            assert_eq!(entry.borrow::<super::HistoryEntry>().icon, icon);
        }
        imp.history_popup.popdown();
        context.block_on(glib::timeout_future(std::time::Duration::from_millis(100)));
        for index in 0..100 {
            let mut entry = activity.clone();
            let mut item = entry.items()[0].clone();
            item.uid = format!("item-{index}");
            entry.set_items(vec![item]);
            status.record(&entry, locale.as_ref());
        }
        assert_eq!(imp.entries().n_items(), 30);
        for (position, uid) in [(0, "item-70"), (29, "item-99")] {
            let entry = imp
                .entries()
                .item(position)
                .and_downcast::<glib::BoxedAnyObject>()
                .unwrap();
            assert!(entry.borrow::<super::HistoryEntry>().text.contains(uid));
        }
        assert!(imp.status.tooltip_text().unwrap().contains("item-99"));
        imp.history_button.popup();
        context.block_on(glib::timeout_future(std::time::Duration::from_millis(150)));
        let adjustment = imp.history_scroll.vadjustment();
        assert!(adjustment.upper() > adjustment.page_size());
        assert!((adjustment.value() - (adjustment.upper() - adjustment.page_size())).abs() < 1.0);
        adjustment.set_value(0.0);
        imp.history_popup.popdown();
        context.block_on(glib::timeout_future(std::time::Duration::from_millis(100)));
        imp.history_button.popup();
        context.block_on(glib::timeout_future(std::time::Duration::from_millis(150)));
        assert!((adjustment.value() - (adjustment.upper() - adjustment.page_size())).abs() < 1.0);
        status.clear(locale.as_ref());
        assert_eq!(imp.entries().n_items(), 0);
        assert!(status.text().is_empty());
        assert!(!imp.history_button.is_sensitive());
        window.close();
        while context.iteration(false) {}
    }
}
