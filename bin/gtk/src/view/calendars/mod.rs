// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use eventix_locale::Locale;
use eventix_view::calendars::{Backend, Calendar, Collection, Snapshot};
use gtk::prelude::*;

mod card;
mod collection;

use crate::model::management::Request;
use card::CalendarCard;
use collection::CollectionActions;

type ActionHandler = Rc<RefCell<Option<Box<dyn Fn(Request)>>>>;

pub(crate) struct CalendarsView {
    pub widget: gtk::Frame,
    pub header: gtk::Box,
    title: gtk::Label,
    empty: gtk::Label,
    collections: gtk::Box,
    actions: gtk::Box,
    scroll: gtk::ScrolledWindow,
    rows: RefCell<Vec<CollectionRow>>,
    language: RefCell<String>,
    toggle_group: gtk::SizeGroup,
    on_action: ActionHandler,
}

struct CollectionRow {
    model: Collection,
    widget: gtk::Grid,
    separator: gtk::Separator,
    description: gtk::Box,
    calendars: gtk::Box,
    cards: Vec<(Calendar, Rc<CalendarCard>)>,
    toggle_group: gtk::SizeGroup,
    on_action: ActionHandler,
    controls: Option<Rc<CollectionActions>>,
}

impl CalendarsView {
    pub fn new(locale: &dyn Locale) -> Self {
        let builder = gtk::Builder::from_resource(resource_path!("view/calendars/content.ui"));
        let view = Self {
            widget: builder.object("content").unwrap(),
            header: builder.object("header").unwrap(),
            title: builder.object("title").unwrap(),
            empty: builder.object("empty").unwrap(),
            collections: builder.object("collections").unwrap(),
            actions: builder.object("actions").unwrap(),
            scroll: builder.object("scroll").unwrap(),
            rows: RefCell::default(),
            language: RefCell::default(),
            toggle_group: gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal),
            on_action: Rc::default(),
        };
        view.title.set_text(locale.translate("Calendars"));
        view.empty.set_text(locale.translate("Loading calendar"));

        view
    }

    pub fn set_snapshot(&self, snapshot: Snapshot) {
        let adjustment = self.scroll.vadjustment();
        let position = adjustment.value();

        let locale = snapshot.locale.as_ref();
        let language = locale.translate("Calendars");
        let translated = *self.language.borrow() != language;
        self.language.replace(language.to_owned());
        self.title.set_text(language);
        self.empty.set_text(locale.translate("No calendars found"));
        self.empty.set_visible(snapshot.collections.is_empty());

        // Reuse live rows and cards so a background sync does not reset text selection or scrolling.
        let mut old = self.rows.take();
        let mut rows = Vec::new();
        let mut previous: Option<gtk::Grid> = None;
        for (index, collection) in snapshot.collections.iter().enumerate() {
            let mut row =
                if let Some(index) = old.iter().position(|row| row.model.id == collection.id) {
                    old.remove(index)
                } else {
                    let row = CollectionRow::new(collection, &self.toggle_group, &self.on_action);
                    self.collections.append(&row.widget);
                    row
                };

            row.update(collection, &snapshot.locale, translated);
            row.separator.set_visible(index > 0);
            self.collections
                .reorder_child_after(&row.widget, previous.as_ref());
            previous = Some(row.widget.clone());
            rows.push(row);
        }

        for row in old {
            self.collections.remove(&row.widget);
        }
        self.rows.replace(rows);
        self.update_actions(&snapshot.collections, locale);

        // Replacing a focused changed card can make GTK reveal another label during allocation.
        // Restore the viewport after allocation, while allowing shorter content to clamp it.
        self.widget.add_tick_callback(move |_, _| {
            adjustment.set_value(position);
            gtk::glib::ControlFlow::Break
        });
    }

    pub fn load_failed(&self, locale: &dyn Locale) {
        if self.rows.borrow().is_empty() {
            self.empty
                .set_text(locale.translate("Could not load local calendars"));
            self.empty.set_visible(true);
        }
    }

    pub fn connect_action(&self, callback: impl Fn(Request) + 'static) {
        self.on_action.replace(Some(Box::new(callback)));
    }

    pub fn finish(&self, request: &Request, success: bool) {
        for row in self
            .rows
            .borrow()
            .iter()
            .filter(|row| row.model.id == request.collection)
        {
            if matches!(
                request.operation,
                crate::model::management::Operation::Collection(_)
            ) {
                if let Some(controls) = &row.controls {
                    controls.finish(request, success);
                }
                continue;
            }
            for (calendar, card) in &row.cards {
                if calendar.id == request.calendar && calendar.configured == request.configured {
                    card.finish(request, success);
                }
            }
        }
    }

    pub fn show_error(&self, request: &Request, message: &str) {
        if matches!(
            request.operation,
            crate::model::management::Operation::Collection(
                crate::model::management::CollectionOperation::Save { .. }
            )
        ) && let Some(controls) = self
            .rows
            .borrow()
            .iter()
            .find(|row| row.model.id == request.collection)
            .and_then(|row| row.controls.as_ref())
        {
            controls.error(message);
        }
    }

    fn update_actions(&self, collections: &[Collection], locale: &dyn Locale) {
        clear(&self.actions);

        let add = action("list-add-symbolic", "Add Collection", locale);
        add.set_halign(gtk::Align::Start);
        let heights = gtk::SizeGroup::new(gtk::SizeGroupMode::Vertical);
        heights.add_widget(&add);
        self.actions.append(&add);

        let choices: Vec<_> = collections
            .iter()
            .filter(|col| col.create_calendar)
            .collect();
        if choices.is_empty() {
            return;
        }

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.set_hexpand(true);
        row.set_halign(gtk::Align::End);
        row.set_valign(gtk::Align::Center);
        row.append(&gtk::Label::new(Some(locale.translate("Calendar Name"))));

        let entry = gtk::Entry::new();
        entry.set_width_chars(15);
        entry.set_max_width_chars(15);
        heights.add_widget(&entry);
        entry.update_property(&[gtk::accessible::Property::Label(
            locale.translate("Calendar Name"),
        )]);
        row.append(&entry);

        let names: Vec<_> = choices.iter().map(|col| col.id.as_str()).collect();
        let select = gtk::DropDown::from_strings(&names);
        heights.add_widget(&select);
        select.set_tooltip_text(Some(locale.translate("Collections")));
        select.update_property(&[gtk::accessible::Property::Label(
            locale.translate("Collections"),
        )]);
        row.append(&select);

        let add_calendar = action("list-add-symbolic", "Add", locale);
        heights.add_widget(&add_calendar);
        row.append(&add_calendar);

        self.actions.append(&row);
    }
}

impl CollectionRow {
    fn new(model: &Collection, toggle_group: &gtk::SizeGroup, on_action: &ActionHandler) -> Self {
        let builder = gtk::Builder::from_resource(resource_path!("view/calendars/collection.ui"));
        Self {
            model: model.clone(),
            widget: builder.object("collection").unwrap(),
            separator: builder.object("separator").unwrap(),
            description: builder.object("description").unwrap(),
            calendars: builder.object("calendars").unwrap(),
            cards: vec![],
            toggle_group: toggle_group.clone(),
            on_action: on_action.clone(),
            controls: None,
        }
    }

    fn update(
        &mut self,
        model: &Collection,
        locale: &Arc<dyn Locale + Send + Sync>,
        translated: bool,
    ) {
        if !translated && self.model == *model && self.description.first_child().is_some() {
            return;
        }

        if translated
            || self.description.first_child().is_none()
            || self.model.backend != model.backend
            || self.model.account != model.account
            || self.model.reload != model.reload
            || self.model.discover != model.discover
        {
            self.update_description(model, locale);
        }
        if let Some(controls) = &self.controls {
            controls.update(model, locale.clone());
        }

        let mut old = std::mem::take(&mut self.cards);
        let mut previous: Option<gtk::Frame> = None;
        for calendar in &model.calendars {
            let reused = old.iter().position(|(old, _)| {
                old.id == calendar.id && old.configured == calendar.configured
            });
            let card = if let Some(index) = reused {
                old.remove(index).1
            } else {
                let card = CalendarCard::new(
                    calendar,
                    model,
                    locale.clone(),
                    &self.toggle_group,
                    &self.on_action,
                );
                self.calendars.append(&card.widget);
                card
            };

            card.update(calendar, model, locale.clone());
            self.calendars
                .reorder_child_after(&card.widget, previous.as_ref());
            previous = Some(card.widget.clone());
            self.cards.push((calendar.clone(), card));
        }

        for (_, card) in old {
            self.calendars.remove(&card.widget);
        }

        self.model = model.clone();
    }

    fn update_description(&mut self, model: &Collection, locale: &Arc<dyn Locale + Send + Sync>) {
        clear(&self.description);
        let locale_ref = locale.as_ref();

        let title = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let (icon, backend) = match model.backend {
            Backend::FileSystem => (resource_path!("icons/fs.png"), "FileSystem"),
            Backend::CalDav => (resource_path!("icons/caldav.png"), "CalDAV"),
            Backend::O365 => (resource_path!("icons/o365.png"), "O365"),
        };
        let image = gtk::Image::from_resource(icon);
        image.set_pixel_size(32);
        image.set_tooltip_text(Some(locale_ref.translate(backend)));
        title.append(&image);

        let name = text(&model.id);
        name.add_css_class("ev_management_collection_name");
        title.append(&name);
        self.description.append(&title);

        if let Some(account) = &model.account {
            self.description.append(&text(account));
        }

        let controls = self
            .controls
            .get_or_insert_with(|| CollectionActions::new(model, locale.clone(), &self.on_action));
        // Reparent stable controls without discarding an open form or a pending action.
        self.description.append(&controls.widget);
    }
}

fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn text(value: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(value)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .selectable(true)
        .build()
}

fn action(icon: &str, label: &str, locale: &dyn Locale) -> gtk::Button {
    action_button(icon, label, true, locale)
}

fn icon_action(icon: &str, label: &str, locale: &dyn Locale) -> gtk::Button {
    action_button(icon, label, false, locale)
}

fn action_button(icon: &str, label: &str, show_label: bool, locale: &dyn Locale) -> gtk::Button {
    let image = if icon == "pencil-symbolic" {
        gtk::Image::from_resource(resource_path!("icons/pencil-symbolic.svg"))
    } else {
        gtk::Image::from_icon_name(icon)
    };
    image.set_pixel_size(14);

    let content: gtk::Widget = if show_label {
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        content.append(&image);
        content.append(&gtk::Label::new(Some(locale.translate(label))));
        content.upcast()
    } else {
        image.upcast()
    };

    let button = gtk::Button::builder()
        .child(&content)
        .valign(gtk::Align::Center)
        .tooltip_text(locale.translate(label))
        .build();
    button.add_css_class("ev_management_button");
    if !show_label {
        button.add_css_class("ev_management_icon_button");
    }
    button.update_property(&[gtk::accessible::Property::Label(locale.translate(label))]);

    button
}

#[cfg(test)]
mod tests {
    use eventix_view::calendars::{Backend, Calendar, Collection, Snapshot};
    use gtk::{gdk, prelude::*};

    use super::CalendarsView;

    fn snapshot() -> Snapshot {
        let mut snapshot = Snapshot {
            collections: vec![Collection {
                id: "CalDAV collection with a long descriptive name".into(),
                syncer: eventix_state::SyncerType::VDirSyncer {
                    email: eventix_state::EmailAccount::new(
                        "User".into(),
                        "user@example.com".into(),
                    ),
                    url: "https://example.com".into(),
                    read_only: false,
                    username: None,
                    password: None,
                    time_span: Default::default(),
                },
                log_path: String::new(),
                backend: Backend::CalDav,
                account: Some("Calendar User <calendar@example.com>".into()),
                reload: true,
                discover: true,
                create_calendar: true,
                calendars: (0..20)
                    .map(|index| Calendar {
                        id: index.to_string(),
                        configured: index != 0,
                        name: "A calendar with a very long display name for wrapping".into(),
                        background: if index == 0 {
                            "invalid color"
                        } else {
                            "#123456"
                        }
                        .into(),
                        foreground: "#ffffff".into(),
                        events: true,
                        tasks: false,
                        personal_alarms: false,
                        default_alarm: None,
                        enabled: index > 1,
                        folder: index.to_string(),
                        path: format!("/long/path/to/a/calendar/collection/{index}"),
                    })
                    .collect(),
            }],
            locale: eventix_locale::default(),
            warnings: vec![],
        };
        snapshot.collections.push(Collection {
            id: "Empty collection".into(),
            syncer: eventix_state::SyncerType::FileSystem {
                path: "/unused".into(),
            },
            log_path: String::new(),
            backend: Backend::FileSystem,
            account: None,
            reload: false,
            discover: false,
            create_calendar: true,
            calendars: vec![],
        });
        snapshot
    }

    fn descendants(widget: &gtk::Widget) -> Vec<gtk::Widget> {
        let mut result = vec![widget.clone()];
        let mut child = widget.first_child();
        while let Some(widget) = child {
            result.extend(descendants(&widget));
            child = widget.next_sibling();
        }

        result
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn calendars_management_renders_actions_and_preserves_cards_and_scroll() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gdk::Display::default().unwrap());

        let view = CalendarsView::new(eventix_locale::default().as_ref());
        view.set_snapshot(snapshot());
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(600)
            .child(&view.widget)
            .build();
        window.present();

        let drain = || {
            for _ in 0..15 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        drain();

        assert!(!view.empty.is_visible());
        assert_eq!(view.actions.orientation(), gtk::Orientation::Horizontal);
        let add_collection = view.actions.first_child().unwrap();
        let calendar_controls = view.actions.last_child().unwrap();
        let controls = descendants(&calendar_controls);
        let entry = controls
            .iter()
            .find_map(|widget| widget.downcast_ref::<gtk::Entry>())
            .unwrap();
        assert_eq!(entry.width_chars(), 15);
        assert_eq!(entry.max_width_chars(), 15);
        assert!(entry.is_sensitive());
        assert!(
            controls
                .iter()
                .filter(|widget| widget.is::<gtk::DropDown>())
                .all(|widget| widget.is_sensitive())
        );

        let separator = view.actions.prev_sibling().unwrap();
        assert!(separator.is::<gtk::Separator>());
        let container = separator.parent().unwrap();
        let separator_bounds = separator.compute_bounds(&container).unwrap();
        assert_eq!(separator_bounds.x(), 0.0);
        assert_eq!(separator_bounds.width(), container.width() as f32);

        let collection_bounds = add_collection.compute_bounds(&view.widget).unwrap();
        let calendar_bounds = calendar_controls.compute_bounds(&view.widget).unwrap();
        assert!(collection_bounds.x() + collection_bounds.width() < calendar_bounds.x());
        assert_eq!(collection_bounds.center().y(), calendar_bounds.center().y());
        for widget in controls.iter().filter(|widget| {
            widget.is::<gtk::Entry>() || widget.is::<gtk::DropDown>() || widget.is::<gtk::Button>()
        }) {
            assert_eq!(
                widget.compute_bounds(&view.widget).unwrap().height(),
                collection_bounds.height()
            );
        }

        let row = view.rows.borrow()[0].widget.clone();
        let title = view.rows.borrow()[0].description.first_child().unwrap();
        assert!(
            title
                .last_child()
                .unwrap()
                .has_css_class("ev_management_collection_name")
        );
        let backend_icon = title
            .first_child()
            .unwrap()
            .downcast::<gtk::Image>()
            .unwrap();
        assert!(backend_icon.icon_name().is_none());
        assert!(backend_icon.paintable().is_some());

        let card = view.rows.borrow()[0].cards[1].1.widget.clone();
        let description = view.rows.borrow()[0]
            .description
            .compute_bounds(&view.widget)
            .unwrap();
        let calendars = view.rows.borrow()[0]
            .calendars
            .compute_bounds(&view.widget)
            .unwrap();
        assert!(description.x() + description.width() <= calendars.x());
        assert!((description.width() / calendars.width() - 2.0 / 3.0).abs() < 0.1);
        assert!(calendars.x() + calendars.width() <= view.widget.width() as f32);
        assert!(!view.rows.borrow()[0].separator.is_visible());
        assert!(view.rows.borrow()[1].separator.is_visible());

        let content = card
            .child()
            .unwrap()
            .first_child()
            .unwrap()
            .downcast::<gtk::Box>()
            .unwrap();
        assert_eq!(content.orientation(), gtk::Orientation::Horizontal);
        let heading = content.first_child().unwrap();
        let actions = content.last_child().unwrap();
        let heading_bounds = heading.compute_bounds(&card).unwrap();
        let action_bounds = actions.compute_bounds(&card).unwrap();
        assert!(heading_bounds.x() + heading_bounds.width() <= action_bounds.x());
        assert!((heading_bounds.center().y() - action_bounds.center().y()).abs() <= 1.0);
        assert!(card.height() <= 44, "calendars stay compact single rows");

        let unknown = view.rows.borrow()[0].cards[0].1.widget.clone();
        let color_x = |card: &gtk::Frame| {
            descendants(card.upcast_ref())
                .into_iter()
                .find_map(|widget| widget.downcast::<gtk::DrawingArea>().ok())
                .unwrap()
                .compute_bounds(&view.widget)
                .unwrap()
                .x()
        };
        assert_eq!(color_x(&unknown), color_x(&card));

        let collection_actions = view.rows.borrow()[0].description.last_child().unwrap();
        assert!(
            collection_actions
                .first_child()
                .unwrap()
                .has_css_class("ev_button_critical")
        );
        assert!(
            collection_actions
                .first_child()
                .unwrap()
                .next_sibling()
                .unwrap()
                .next_sibling()
                .unwrap()
                .has_css_class("ev_button_warn")
        );
        assert_collection_buttons(&collection_actions);

        let widgets = descendants(view.widget.upcast_ref());
        let buttons: Vec<_> = widgets
            .iter()
            .filter_map(|widget| widget.downcast_ref::<gtk::Button>())
            .collect();
        assert!(buttons.len() > 20);
        assert!(
            buttons
                .iter()
                .all(|button| button.is_sensitive() && button.action_name().is_none())
        );

        let labels: Vec<_> = widgets
            .iter()
            .filter_map(|widget| widget.downcast_ref::<gtk::Label>())
            .map(|label| label.text())
            .collect();
        for action in ["Reload", "Discover", "Sync", "Log", "Add Collection", "Add"] {
            assert!(labels.iter().any(|label| label == action), "{action}");
        }
        for removed in ["Delete", "Edit", "Folder", "Unconfigured"] {
            assert!(!labels.iter().any(|label| label == removed), "{removed}");
        }
        assert!(!labels.iter().any(|label| label.starts_with("/long/path")));

        for label in ["Delete", "Edit"] {
            let button = buttons
                .iter()
                .find(|button| button.tooltip_text().as_deref() == Some(label))
                .unwrap();
            assert!(
                descendants(button.upcast_ref())
                    .iter()
                    .all(|widget| !widget.is::<gtk::Label>())
            );
            assert!(button.height() <= 30);
            let image = button.child().unwrap().downcast::<gtk::Image>().unwrap();
            let bounds = image.compute_bounds(*button).unwrap();
            let button_bounds = button.compute_bounds(*button).unwrap();
            assert!((bounds.center().x() - button_bounds.center().x()).abs() <= 0.5);
            if label == "Edit" {
                assert!(image.icon_name().is_none());
                assert!(
                    image.paintable().is_some(),
                    "edit uses the bundled pencil image"
                );
            }
        }

        let switches: Vec<_> = widgets
            .iter()
            .filter_map(|widget| widget.downcast_ref::<gtk::Switch>())
            .collect();
        assert_eq!(switches.len(), 19);
        assert!(!switches[0].is_active());
        assert!(switches[1].is_active());
        assert!(switches.iter().all(|switch| switch.is_sensitive()));

        let scroll = widgets
            .iter()
            .find_map(|widget| widget.downcast_ref::<gtk::ScrolledWindow>())
            .unwrap();
        assert!(scroll.vadjustment().upper() > scroll.vadjustment().page_size());
        assert!(!view.actions.is_ancestor(scroll));
        let footer_bounds = view.actions.compute_bounds(&view.widget).unwrap();
        let scroll_bounds = scroll.compute_bounds(&view.widget).unwrap();
        assert!(footer_bounds.y() >= scroll_bounds.y() + scroll_bounds.height());

        scroll.vadjustment().set_value(100.0);
        view.set_snapshot(snapshot());
        drain();
        assert_eq!(view.rows.borrow()[0].widget, row);
        assert_eq!(view.rows.borrow()[0].cards[1].1.widget, card);
        assert_eq!(scroll.vadjustment().value(), 100.0);
        assert_eq!(
            view.actions.compute_bounds(&view.widget).unwrap(),
            footer_bounds
        );

        let mut updated = snapshot();
        updated.collections[0].calendars[0].name = "Renamed".into();
        view.set_snapshot(updated);
        drain();
        assert_eq!(view.rows.borrow()[0].cards[1].1.widget, card);
        assert_eq!(scroll.vadjustment().value(), 100.0);

        let mut empty = snapshot();
        empty.collections.clear();
        view.set_snapshot(empty);
        assert!(view.empty.is_visible());
        assert_eq!(view.empty.text(), "No calendars found");
        assert!(view.collections.first_child().is_none());

        let dir = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(dir.path(), dir.path());
        let mut german = snapshot();
        german.locale = eventix_locale::new_with_timezone(
            &xdg,
            eventix_locale::LocaleType::German,
            chrono_tz::UTC,
        )
        .unwrap();
        view.set_snapshot(german);
        assert_eq!(view.title.text(), "Kalender");
        let widgets = descendants(view.widget.upcast_ref());
        assert!(
            widgets
                .iter()
                .filter_map(|widget| widget.downcast_ref::<gtk::Label>())
                .all(|label| label.text() != "Nicht konfiguriert")
        );

        let mut reordered = snapshot();
        let unconfigured = reordered.collections[0].calendars[0].clone();
        reordered.collections[1].calendars.push(unconfigured);
        reordered.collections.reverse();
        view.set_snapshot(reordered);
        drain();
        assert!(!view.rows.borrow()[0].separator.is_visible());
        assert!(view.rows.borrow()[1].separator.is_visible());
        assert_eq!(
            color_x(&view.rows.borrow()[0].cards[0].1.widget),
            color_x(&view.rows.borrow()[1].cards[1].1.widget)
        );

        window.set_default_size(400, 600);
        drain();
        assert!(
            window.width() > 400,
            "collection actions impose a minimum window width"
        );
        let collection_actions = view.rows.borrow()[1].description.last_child().unwrap();
        assert_collection_buttons(&collection_actions);

        window.close();
    }

    fn assert_collection_buttons(actions: &gtk::Widget) {
        assert!(actions.is::<gtk::Box>());
        assert_eq!(actions.halign(), gtk::Align::Start);

        let mut child = actions.first_child();
        let mut previous_right = 0.0;
        let mut center = None;
        while let Some(widget) = child {
            let button = widget.clone().downcast::<gtk::Button>().unwrap();
            let bounds = button.compute_bounds(actions).unwrap();
            assert_eq!(
                bounds.width() as i32,
                button.measure(gtk::Orientation::Horizontal, -1).1
            );
            assert!(bounds.x() >= previous_right);
            assert_eq!(
                *center.get_or_insert(bounds.center().y()),
                bounds.center().y()
            );
            previous_right = bounds.x() + bounds.width();
            child = widget.next_sibling();
        }
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn calendars_management_actions_submit_drafts_and_recover_from_failures() {
        use std::{cell::RefCell, rc::Rc};

        use crate::model::management::{Operation, Request};

        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let view = CalendarsView::new(eventix_locale::default().as_ref());
        let requests = Rc::new(RefCell::new(Vec::<Request>::new()));
        let received = requests.clone();
        view.connect_action(move |request| received.borrow_mut().push(request));
        let mut initial = snapshot();
        initial.collections[0].calendars.truncate(2);
        view.set_snapshot(initial);
        let card = view.rows.borrow()[0].cards[1].1.widget.clone();
        let button = |tooltip: &str| {
            descendants(card.upcast_ref())
                .into_iter()
                .filter_map(|widget| widget.downcast::<gtk::Button>().ok())
                .find(|button| button.tooltip_text().as_deref() == Some(tooltip))
                .unwrap()
        };
        let switch = descendants(card.upcast_ref())
            .into_iter()
            .find_map(|widget| widget.downcast::<gtk::Switch>().ok())
            .unwrap();
        switch.set_active(true);
        let request = requests.borrow_mut().pop().unwrap();
        assert!(matches!(request.operation, Operation::Enable(true)));
        assert_eq!(request.calendar, "1");
        assert!(!card.is_sensitive());
        view.finish(&request, false);
        assert!(card.is_sensitive());
        assert!(!switch.is_active());
        assert!(
            requests.borrow().is_empty(),
            "rollback does not submit another mutation"
        );

        button("Edit").emit_clicked();
        let widgets = descendants(card.upcast_ref());
        let name = widgets
            .iter()
            .filter_map(|widget| widget.downcast_ref::<gtk::Entry>())
            .find(|entry| entry.tooltip_text().as_deref() == Some("Calendar Name"))
            .unwrap();
        name.set_text("Saved name");
        let colors: Vec<_> = widgets
            .iter()
            .filter_map(|widget| widget.downcast_ref::<gtk::ColorDialogButton>())
            .collect();
        colors[0].set_rgba(&gdk::RGBA::RED);
        button("Save").emit_clicked();
        let request = requests.borrow_mut().pop().unwrap();
        let Operation::Save(settings) = &request.operation else {
            panic!("expected save");
        };
        assert_eq!(settings.name(), "Saved name");
        assert_eq!(settings.bgcolor(), "#ff0000");
        assert_eq!(settings.folder(), "1");
        assert_eq!(
            settings.types(),
            &vec![eventix_ical::objects::CalCompType::Event]
        );
        let reveal = widgets
            .iter()
            .find_map(|widget| widget.downcast_ref::<gtk::Revealer>())
            .unwrap();
        view.finish(&request, false);
        assert!(reveal.reveals_child());
        assert_eq!(name.text(), "Saved name");
        button("Save").emit_clicked();
        let request = requests.borrow_mut().pop().unwrap();
        view.finish(&request, true);
        assert!(!reveal.reveals_child());

        let mut refreshed = snapshot();
        refreshed.collections[0].calendars[1].enabled = true;
        view.set_snapshot(refreshed);
        assert!(switch.is_active());
        assert!(
            requests.borrow().is_empty(),
            "snapshot updates never submit mutations"
        );

        let mut local = snapshot();
        local.collections[0].backend = Backend::FileSystem;
        view.set_snapshot(local);
        for (tooltip, reload) in [("Reload", true), ("Delete", false)] {
            button(tooltip).emit_clicked();
            let request = requests.borrow_mut().pop().unwrap();
            assert_eq!(matches!(request.operation, Operation::Reload), reload);
            assert_eq!(matches!(request.operation, Operation::Delete), !reload);
            view.finish(&request, true);
        }
        let folder = descendants(card.upcast_ref())
            .into_iter()
            .filter_map(|widget| widget.downcast::<gtk::Button>().ok())
            .find(|button| {
                button.tooltip_text().as_deref() == Some("/long/path/to/a/calendar/collection/1")
            })
            .unwrap();
        folder.emit_clicked();
        let copied = gtk::glib::MainContext::default()
            .block_on(card.clipboard().read_text_future())
            .unwrap();
        assert_eq!(
            copied.as_deref(),
            Some("/long/path/to/a/calendar/collection/1")
        );

        let unknown = view.rows.borrow()[0].cards[0].1.widget.clone();
        let click = |tooltip: &str| {
            descendants(unknown.upcast_ref())
                .into_iter()
                .filter_map(|widget| widget.downcast::<gtk::Button>().ok())
                .find(|button| button.tooltip_text().as_deref() == Some(tooltip))
                .unwrap()
                .emit_clicked();
        };
        click("Add");
        click("Save");
        let request = requests.borrow_mut().pop().unwrap();
        assert!(!request.configured);
        assert_eq!(request.calendar, "0");
        assert!(matches!(request.operation, Operation::Save(_)));
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn collection_actions_submit_recover_and_preserve_edit_drafts() {
        use crate::model::management::{CollectionOperation, Operation, Request};
        use std::{cell::RefCell, rc::Rc};

        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gdk::Display::default().unwrap());
        let view = CalendarsView::new(eventix_locale::default().as_ref());
        let requests = Rc::new(RefCell::new(Vec::<Request>::new()));
        let received = requests.clone();
        view.connect_action(move |request| received.borrow_mut().push(request));
        view.set_snapshot(snapshot());
        let window = gtk::Window::builder().child(&view.widget).build();
        window.present();
        let controls = view.rows.borrow()[0].controls.as_ref().unwrap().clone();
        let button = |root: &gtk::Widget, tooltip: &str| {
            descendants(root)
                .into_iter()
                .filter_map(|widget| widget.downcast::<gtk::Button>().ok())
                .find(|button| button.tooltip_text().as_deref() == Some(tooltip))
                .unwrap()
        };
        for action in ["Sync", "Discover"] {
            button(controls.widget.upcast_ref(), action).emit_clicked();
            let request = requests.borrow_mut().pop().unwrap();
            assert_eq!(request.collection, snapshot().collections[0].id);
            assert!(request.calendar.is_empty());
            assert!(matches!(
                (&request.operation, action),
                (Operation::Collection(CollectionOperation::Sync), "Sync")
                    | (
                        Operation::Collection(CollectionOperation::Discover),
                        "Discover"
                    )
            ));
            assert!(!controls.widget.is_sensitive());
            view.finish(&request, false);
            assert!(controls.widget.is_sensitive());
        }
        button(controls.widget.upcast_ref(), "Edit").emit_clicked();
        let editor = controls.editor.borrow().as_ref().unwrap().clone();
        assert!(editor.is_visible());
        assert!(editor.is_modal());
        assert_eq!(editor.transient_for(), Some(window.clone()));
        assert_eq!(editor.title().as_deref(), Some("Edit Collection"));
        for _ in 0..10 {
            while gtk::glib::MainContext::default().iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            descendants(view.rows.borrow()[0].description.upcast_ref())
                .iter()
                .all(|widget| !widget.is::<gtk::Entry>())
        );
        let fields = descendants(editor.upcast_ref());
        let collection_name = fields
            .iter()
            .filter_map(|widget| widget.downcast_ref::<gtk::Label>())
            .find(|label| label.text() == snapshot().collections[0].id)
            .unwrap();
        assert!(collection_name.selection_bounds().is_none());
        let main_rows: Vec<_> = fields
            .iter()
            .filter(|widget| widget.has_css_class("ev_management_main_row"))
            .collect();
        assert_eq!(main_rows.len(), 2);
        let main_label = main_rows[0].first_child().unwrap().last_child().unwrap();
        let label_x = main_label.compute_bounds(&editor).unwrap().x();
        for main in &main_rows {
            let heading = main.first_child().unwrap();
            assert!(heading.first_child().unwrap().is::<gtk::Image>());
            assert_eq!(
                heading
                    .last_child()
                    .unwrap()
                    .compute_bounds(&editor)
                    .unwrap()
                    .x(),
                label_x
            );
        }
        let selector = main_rows[1].last_child().unwrap();
        assert!(selector.height() <= 28, "sync-type selector stays compact");
        let bounds = selector.compute_bounds(&editor).unwrap();
        let row_bounds = main_rows[1].compute_bounds(&editor).unwrap();
        assert_eq!(
            bounds.x() + bounds.width(),
            row_bounds.x() + row_bounds.width()
        );
        let identity = main_rows[0].parent().unwrap();
        assert!(
            descendants(&identity)
                .iter()
                .all(|widget| !widget.is::<gtk::Separator>())
        );
        let content = identity.parent().unwrap();
        let syncer_section = main_rows[1].parent().unwrap();
        assert!(main_rows[0].has_css_class("ev_management_hover_area"));
        assert!(syncer_section.has_css_class("ev_management_hover_area"));
        assert_eq!(syncer_section.parent(), Some(content.clone()));
        assert_eq!(content.margin_start(), 8);
        assert_eq!(content.margin_end(), 8);
        assert_eq!(content.margin_top(), 8);
        assert_eq!(content.margin_bottom(), 8);
        assert!(!content.has_css_class("ev_management_form"));
        for row in fields
            .iter()
            .filter(|widget| widget.has_css_class("ev_management_sync_row"))
        {
            assert_eq!(row.parent(), Some(syncer_section.clone()));
            let heading = row.first_child().unwrap();
            let label = heading.first_child().unwrap();
            assert!(label.is::<gtk::Label>());
            assert_eq!(label.compute_bounds(&editor).unwrap().x(), label_x);
            assert_eq!(
                row.last_child()
                    .unwrap()
                    .compute_bounds(&editor)
                    .unwrap()
                    .x(),
                main_rows[0]
                    .last_child()
                    .unwrap()
                    .compute_bounds(&editor)
                    .unwrap()
                    .x(),
                "collection name aligns with synchronizer fields"
            );
        }
        let checks: Vec<_> = fields
            .iter()
            .filter_map(|widget| widget.downcast_ref::<gtk::CheckButton>())
            .collect();
        assert_eq!(checks.len(), 3);
        let indicator_x = checks[0]
            .first_child()
            .unwrap()
            .compute_bounds(&editor)
            .unwrap()
            .x();
        assert_eq!(
            checks[1]
                .first_child()
                .unwrap()
                .compute_bounds(&editor)
                .unwrap()
                .x(),
            indicator_x
        );
        let spinner = fields
            .iter()
            .find_map(|widget| widget.downcast_ref::<gtk::SpinButton>())
            .unwrap();
        for radio in &checks[1..] {
            assert_eq!(
                radio.compute_bounds(&editor).unwrap().center().y(),
                spinner.compute_bounds(&editor).unwrap().center().y()
            );
        }
        let name = fields
            .iter()
            .filter_map(|widget| widget.downcast_ref::<gtk::Entry>())
            .find(|entry| entry.tooltip_text().as_deref() == Some("Your Name"))
            .unwrap();
        name.set_text("");
        button(editor.upcast_ref(), "Save").emit_clicked();
        assert!(requests.borrow().is_empty());
        let error = descendants(editor.upcast_ref())
            .into_iter()
            .find_map(|widget| {
                widget
                    .downcast::<gtk::Label>()
                    .ok()
                    .filter(|label| label.has_css_class("ev_management_error"))
            })
            .unwrap();
        assert!(error.is_visible());
        assert!(
            error
                .text()
                .contains(snapshot().locale.translate("error.collection_your_name"))
        );
        name.set_text("Unsaved collection draft");
        let mut updated = snapshot();
        updated.collections[0].account = Some("Changed externally".into());
        view.set_snapshot(updated);
        assert_eq!(
            view.rows.borrow()[0].controls.as_ref().unwrap().widget,
            controls.widget
        );
        assert_eq!(name.text(), "Unsaved collection draft");
        assert_eq!(controls.editor.borrow().as_ref(), Some(&editor));
        let keys = editor
            .observe_controllers()
            .snapshot()
            .into_iter()
            .find_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
            .unwrap();
        assert!(keys.emit_by_name::<bool>(
            "key-pressed",
            &[&gdk::Key::Return, &0u32, &gdk::ModifierType::CONTROL_MASK,]
        ));
        assert!(!error.is_visible());
        let request = requests.borrow_mut().pop().unwrap();
        assert!(!editor.is_sensitive());
        editor.close();
        assert!(
            controls.editor.borrow().is_some(),
            "pending saves cannot be dismissed"
        );
        let Operation::Collection(CollectionOperation::Save { syncer, password }) =
            &request.operation
        else {
            panic!("expected collection save");
        };
        assert_eq!(syncer.email().unwrap().name(), "Unsaved collection draft");
        assert!(password.is_none());
        view.finish(&request, false);
        view.show_error(&request, "Test persistence failure");
        assert!(error.is_visible());
        assert!(error.text().contains("Test persistence failure"));
        assert!(editor.is_visible());
        assert!(editor.is_sensitive());
        assert_eq!(name.text(), "Unsaved collection draft");
        button(editor.upcast_ref(), "Save").emit_clicked();
        let request = requests.borrow_mut().pop().unwrap();
        view.finish(&request, true);
        assert!(controls.editor.borrow().is_none());
        assert!(!editor.is_visible());
        button(controls.widget.upcast_ref(), "Edit").emit_clicked();
        let editor = controls.editor.borrow().as_ref().unwrap().clone();
        button(editor.upcast_ref(), "Cancel").emit_clicked();
        assert!(controls.editor.borrow().is_none());
        assert!(!editor.is_visible());
        button(controls.widget.upcast_ref(), "Edit").emit_clicked();
        let editor = controls.editor.borrow().as_ref().unwrap().clone();
        editor.close();
        assert!(controls.editor.borrow().is_none());
        assert!(requests.borrow().is_empty());
        window.close();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn collection_delete_reload_confirm_and_log_displays_contents() {
        use crate::model::management::{CollectionOperation, Operation, Request};
        use std::{cell::RefCell, rc::Rc};

        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let view = CalendarsView::new(eventix_locale::default().as_ref());
        let requests = Rc::new(RefCell::new(Vec::<Request>::new()));
        let received = requests.clone();
        view.connect_action(move |request| received.borrow_mut().push(request));
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("collection.log");
        std::fs::write(&log, "Collection sync log\nTest entry").unwrap();
        let mut initial = snapshot();
        initial.collections[0].calendars.clear();
        initial.collections[0].log_path = log.to_string_lossy().into_owned();
        view.set_snapshot(initial);
        let window = gtk::Window::builder().child(&view.widget).build();
        window.present();
        let drain = || {
            for _ in 0..15 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        let controls = view.rows.borrow()[0].controls.as_ref().unwrap().clone();
        for (action, confirm) in [("Delete", false), ("Delete", true), ("Reload", true)] {
            descendants(controls.widget.upcast_ref())
                .into_iter()
                .filter_map(|widget| widget.downcast::<gtk::Button>().ok())
                .find(|button| button.tooltip_text().as_deref() == Some(action))
                .unwrap()
                .emit_clicked();
            drain();
            assert!(requests.borrow().is_empty());
            let dialog = gtk::Window::list_toplevels()
                .into_iter()
                .filter_map(|widget| widget.downcast::<gtk::Window>().ok())
                .find(|candidate| candidate != &window && candidate.is_visible())
                .unwrap();
            descendants(dialog.upcast_ref())
                .into_iter()
                .filter_map(|widget| widget.downcast::<gtk::Button>().ok())
                .find(|button| {
                    button.label().as_deref() == Some(if confirm { action } else { "Cancel" })
                })
                .unwrap()
                .emit_clicked();
            drain();
            if confirm {
                let request = requests.borrow_mut().pop().unwrap();
                assert!(matches!(
                    (&request.operation, action),
                    (Operation::Collection(CollectionOperation::Delete), "Delete")
                        | (Operation::Collection(CollectionOperation::Reload), "Reload")
                ));
                view.finish(&request, true);
            } else {
                assert!(requests.borrow().is_empty());
            }
        }
        descendants(controls.widget.upcast_ref())
            .into_iter()
            .filter_map(|widget| widget.downcast::<gtk::Button>().ok())
            .find(|button| button.tooltip_text().as_deref() == Some("Log"))
            .unwrap()
            .emit_clicked();
        drain();
        let log_window = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|widget| widget.downcast::<gtk::Window>().ok())
            .find(|candidate| {
                candidate
                    .title()
                    .is_some_and(|title| title.starts_with("Log of collection"))
            })
            .unwrap();
        let text = descendants(log_window.upcast_ref())
            .into_iter()
            .find_map(|widget| widget.downcast::<gtk::TextView>().ok())
            .unwrap();
        let buffer = text.buffer();
        assert_eq!(
            buffer.text(&buffer.start_iter(), &buffer.end_iter(), false),
            "Collection sync log\nTest entry"
        );
        let keys = log_window
            .observe_controllers()
            .snapshot()
            .into_iter()
            .find_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
            .unwrap();
        assert_eq!(keys.propagation_phase(), gtk::PropagationPhase::Capture);
        assert!(!keys.emit_by_name::<bool>(
            "key-pressed",
            &[&gdk::Key::a, &0u32, &gdk::ModifierType::empty()],
        ));
        assert!(log_window.is_visible());
        assert!(keys.emit_by_name::<bool>(
            "key-pressed",
            &[&gdk::Key::Escape, &0u32, &gdk::ModifierType::empty()],
        ));
        assert!(!log_window.is_visible());
        window.close();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn calendars_management_edits_and_adds_animate_and_preserve_drafts_on_refresh() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gdk::Display::default().unwrap());
        gtk::Settings::default()
            .unwrap()
            .set_gtk_enable_animations(true);

        let mut initial = snapshot();
        initial.collections[0].calendars.truncate(2);
        let configured = &mut initial.collections[0].calendars[1];
        configured.personal_alarms = true;
        configured.default_alarm = Some(eventix_ical::objects::CalAlarm::new(
            eventix_ical::objects::CalAction::Display,
            eventix_ical::objects::CalTrigger::Relative {
                duration: chrono::Duration::minutes(-30).into(),
                related: eventix_ical::objects::CalRelated::End,
            },
        ));
        let view = CalendarsView::new(initial.locale.as_ref());
        view.set_snapshot(initial);
        let window = gtk::Window::builder()
            .default_width(1400)
            .default_height(900)
            .child(&view.widget)
            .build();
        window.present();
        let drain = |milliseconds| {
            for _ in 0..milliseconds / 10 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        let button = |card: &gtk::Frame, tooltip: &str| {
            descendants(card.upcast_ref())
                .into_iter()
                .filter_map(|widget| widget.downcast::<gtk::Button>().ok())
                .find(|button| button.tooltip_text().as_deref() == Some(tooltip))
                .unwrap()
        };
        let revealer = |card: &gtk::Frame| {
            descendants(card.upcast_ref())
                .into_iter()
                .find_map(|widget| widget.downcast::<gtk::Revealer>().ok())
                .unwrap()
        };
        let entry = |card: &gtk::Frame, tooltip: &str| {
            descendants(card.upcast_ref())
                .into_iter()
                .filter_map(|widget| widget.downcast::<gtk::Entry>().ok())
                .find(|entry| entry.tooltip_text().as_deref() == Some(tooltip))
                .unwrap()
        };
        drain(100);
        let card = view.rows.borrow()[0].cards[1].1.widget.clone();
        let unknown = view.rows.borrow()[0].cards[0].1.widget.clone();
        let edit = button(&card, "Edit");
        let reveal = revealer(&card);
        let collapsed_height = card.height();
        let header = card.child().unwrap().first_child().unwrap();
        let header_height = header.height();
        let unknown_header = unknown.child().unwrap().first_child().unwrap();
        let unknown_header_height = unknown_header.height();
        let title = entry(&card, "Calendar Name").parent().unwrap();
        let title_x = title.compute_bounds(&card).unwrap().x();
        let unknown_title = entry(&unknown, "Calendar Name").parent().unwrap();
        let unknown_title_x = unknown_title.compute_bounds(&unknown).unwrap().x();
        let header_controls = descendants(&header);
        let toggle = header_controls
            .iter()
            .find_map(|widget| widget.downcast_ref::<gtk::Switch>())
            .unwrap();
        let dot = header_controls
            .iter()
            .find_map(|widget| widget.downcast_ref::<gtk::DrawingArea>())
            .unwrap();
        let toggle_bounds = toggle.compute_bounds(&card).unwrap();
        let dot_bounds = dot.compute_bounds(&card).unwrap();
        assert!(dot_bounds.x() - toggle_bounds.x() - toggle_bounds.width() >= 16.0);
        assert!(!reveal.reveals_child());
        edit.emit_clicked();
        assert!(reveal.reveals_child());
        assert_eq!(
            reveal.transition_type(),
            gtk::RevealerTransitionType::SlideDown
        );
        assert_eq!(reveal.transition_duration(), 300);
        assert!(!reveal.is_child_revealed(), "opening is animated");
        let heights = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let frames = heights.clone();
        let callback = card.add_tick_callback(move |card, _| {
            frames.borrow_mut().push(card.height());
            gtk::glib::ControlFlow::Continue
        });
        drain(450);
        callback.remove();
        assert!(reveal.is_child_revealed());
        assert_eq!(
            title.compute_bounds(&card).unwrap().x(),
            title_x,
            "first expansion keeps the title column in place"
        );
        assert_eq!(
            header.height(),
            header_height,
            "editing keeps the title row height"
        );
        assert!(
            heights
                .borrow()
                .iter()
                .any(|height| *height > collapsed_height && *height < card.height()),
            "opening has intermediate sizes"
        );
        let name = entry(&card, "Calendar Name");
        let folder = entry(&card, "Folder");
        assert_eq!(folder.text(), "1");
        assert!(!folder.is_editable());
        let controls = descendants(reveal.child().unwrap().upcast_ref());
        let separators: Vec<_> = controls
            .iter()
            .filter(|widget| widget.has_css_class("ev_management_form_separator"))
            .collect();
        assert_eq!(separators.len(), 3);
        assert!(separators.iter().all(|separator| separator.height() == 1));
        let labels: Vec<_> = controls
            .iter()
            .filter_map(|widget| widget.downcast_ref::<gtk::Label>())
            .filter(|label| label.has_css_class("ev_management_form_label"))
            .collect();
        assert_eq!(labels.len(), 4);
        for label in labels {
            assert_eq!(label.valign(), gtk::Align::Center);
            assert_eq!(
                label.pango_context().font_description().unwrap().weight(),
                gtk::pango::Weight::Bold
            );
            let row = label.parent().unwrap();
            let bounds = label.compute_bounds(&row).unwrap();
            assert!((bounds.center().y() - row.height() as f32 / 2.0).abs() <= 0.5);
            let field = label.next_sibling().unwrap();
            assert_eq!(
                field.compute_bounds(&card).unwrap().x(),
                name.compute_bounds(&card).unwrap().x(),
                "{} field aligns with the calendar name",
                label.text(),
            );
        }
        let duration = controls
            .iter()
            .find_map(|widget| widget.downcast_ref::<gtk::SpinButton>())
            .unwrap();
        assert_eq!(duration.value_as_int(), 30);
        let colors: Vec<_> = controls
            .iter()
            .filter_map(|widget| widget.downcast_ref::<gtk::ColorDialogButton>())
            .collect();
        assert_eq!(colors.len(), 2);
        assert_eq!(colors[0].rgba(), "#123456".parse::<gdk::RGBA>().unwrap());
        assert_eq!(colors[1].rgba(), gdk::RGBA::WHITE);
        let color_grid = colors[0]
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .downcast::<gtk::Grid>()
            .unwrap();
        for (row, title) in ["Preview (timed):", "Preview (all-day):"]
            .into_iter()
            .enumerate()
        {
            let row = row as i32;
            assert_eq!(color_grid.child_at(0, row), colors[row as usize].parent());
            let label = color_grid
                .child_at(1, row)
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap();
            assert_eq!(label.text(), title);
            assert_eq!(label.xalign(), 0.0);
            let picker_bounds = colors[row as usize].compute_bounds(&color_grid).unwrap();
            let label_bounds = label.compute_bounds(&color_grid).unwrap();
            assert!(label_bounds.x() - picker_bounds.x() - picker_bounds.width() >= 32.0);
            assert_eq!(
                label.pango_context().font_description().unwrap().weight(),
                gtk::pango::Weight::Bold
            );
            let preview = color_grid.child_at(2, row).unwrap();
            assert!(preview.is::<gtk::DrawingArea>());
            assert!(color_grid.child_at(3, row).is_none());
            if row == 1 {
                assert!(preview.width() < 90);
                assert_eq!(preview.height(), 20);
            }
        }
        for (label, active) in [("Events", true), ("Tasks", false)] {
            let check = controls
                .iter()
                .filter_map(|widget| widget.downcast_ref::<gtk::CheckButton>())
                .find(|check| check.label().as_deref() == Some(label))
                .unwrap();
            assert_eq!(check.is_active(), active);
        }
        let personal = controls
            .iter()
            .filter_map(|widget| widget.downcast_ref::<gtk::ToggleButton>())
            .find(|button| button.label().as_deref() == Some("Personal"))
            .unwrap();
        assert!(personal.is_active());
        assert!(duration.is_mapped());
        name.set_text("Unsaved draft");
        colors[0].set_rgba(&gdk::RGBA::RED);
        let mut refreshed = snapshot();
        refreshed.collections[0].calendars.truncate(2);
        refreshed.collections[0].calendars[1].name = "Changed externally".into();
        view.set_snapshot(refreshed);
        drain(50);
        assert_eq!(view.rows.borrow()[0].cards[1].1.widget, card);
        assert_eq!(entry(&card, "Calendar Name"), name);
        assert_eq!(name.text(), "Unsaved draft");
        assert!(reveal.is_child_revealed());

        button(&unknown, "Add").emit_clicked();
        drain(350);
        assert!(revealer(&unknown).is_child_revealed());
        assert_eq!(
            unknown_title.compute_bounds(&unknown).unwrap().x(),
            unknown_title_x,
            "first expansion keeps the unconfigured title column in place"
        );
        let unknown_name = entry(&unknown, "Calendar Name");
        let unknown_folder = entry(&unknown, "Folder");
        assert_eq!(
            unknown_folder.compute_bounds(&unknown).unwrap().x(),
            unknown_name.compute_bounds(&unknown).unwrap().x(),
        );
        assert_eq!(
            unknown_header.height(),
            unknown_header_height,
            "adding keeps the title row height"
        );
        assert!(reveal.is_child_revealed(), "cards expand independently");
        button(&card, "Save").emit_clicked();
        assert!(reveal.reveals_child(), "Save needs a connected session");
        let expanded_height = card.height();
        heights.borrow_mut().clear();
        let frames = heights.clone();
        let callback = card.add_tick_callback(move |card, _| {
            frames.borrow_mut().push(card.height());
            gtk::glib::ControlFlow::Continue
        });
        button(&card, "Cancel").emit_clicked();
        drain(100);
        assert!(
            reveal.child().is_some(),
            "form stays alive while collapsing"
        );
        drain(350);
        callback.remove();
        assert!(!reveal.is_child_revealed());
        assert_eq!(card.height(), collapsed_height);
        assert_eq!(header.height(), header_height);
        assert!(
            heights
                .borrow()
                .iter()
                .any(|height| *height > collapsed_height && *height < expanded_height),
            "closing has intermediate sizes"
        );
        assert!(edit.has_focus());
        edit.emit_clicked();
        assert_eq!(name.text(), "Changed externally");
        // Reversing a partly completed opening must not destroy the closing animation.
        drain(100);
        button(&card, "Cancel").emit_clicked();
        assert!(reveal.child().is_some());
        drain(350);
        assert!(!reveal.is_child_revealed());

        let mut local = snapshot();
        local.collections[0].backend = Backend::FileSystem;
        local.collections[0].calendars.truncate(2);
        view.set_snapshot(local);
        edit.emit_clicked();
        assert!(entry(&card, "Folder").is_editable());
        button(&card, "Cancel").emit_clicked();
        drain(350);
        gtk::Settings::default()
            .unwrap()
            .set_gtk_enable_animations(false);
        edit.emit_clicked();
        drain(50);
        assert!(
            reveal.is_child_revealed(),
            "reveal={} mapped={} animations={}",
            reveal.reveals_child(),
            reveal.is_mapped(),
            gtk::Settings::default().unwrap().is_gtk_enable_animations()
        );
        button(&card, "Cancel").emit_clicked();
        drain(50);
        assert!(!reveal.is_child_revealed());
        window.close();
    }
}
