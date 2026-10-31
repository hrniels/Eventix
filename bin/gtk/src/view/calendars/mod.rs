// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;

use eventix_locale::Locale;
use eventix_view::calendars::{Backend, Calendar, Collection, Snapshot};
use gtk::{gdk, prelude::*};

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
}

struct CollectionRow {
    model: Collection,
    widget: gtk::Grid,
    separator: gtk::Separator,
    description: gtk::Box,
    calendars: gtk::Box,
    cards: Vec<(Calendar, gtk::Frame)>,
    toggle_group: gtk::SizeGroup,
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
                    let row = CollectionRow::new(collection, &self.toggle_group);
                    self.collections.append(&row.widget);
                    row
                };

            row.update(collection, locale, translated);
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
    fn new(model: &Collection, toggle_group: &gtk::SizeGroup) -> Self {
        let builder = gtk::Builder::from_resource(resource_path!("view/calendars/collection.ui"));
        Self {
            model: model.clone(),
            widget: builder.object("collection").unwrap(),
            separator: builder.object("separator").unwrap(),
            description: builder.object("description").unwrap(),
            calendars: builder.object("calendars").unwrap(),
            cards: vec![],
            toggle_group: toggle_group.clone(),
        }
    }

    fn update(&mut self, model: &Collection, locale: &dyn Locale, translated: bool) {
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

        let rebuild = translated
            || self.model.backend != model.backend
            || self.model.create_calendar != model.create_calendar;
        let mut old = std::mem::take(&mut self.cards);
        let mut previous: Option<gtk::Frame> = None;
        for calendar in &model.calendars {
            let reused = old.iter().position(|(old, _)| {
                old.id == calendar.id
                    && old.configured == calendar.configured
                    && *old == *calendar
                    && !rebuild
            });
            let card = if let Some(index) = reused {
                old.remove(index).1
            } else {
                let card = calendar_card(calendar, model, locale, &self.toggle_group);
                self.calendars.append(&card);
                card
            };

            self.calendars.reorder_child_after(&card, previous.as_ref());
            previous = Some(card.clone());
            self.cards.push((calendar.clone(), card));
        }

        for (_, card) in old {
            self.calendars.remove(&card);
        }

        self.model = model.clone();
    }

    fn update_description(&self, model: &Collection, locale: &dyn Locale) {
        clear(&self.description);

        let title = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let (icon, backend) = match model.backend {
            Backend::FileSystem => (resource_path!("icons/fs.png"), "FileSystem"),
            Backend::CalDav => (resource_path!("icons/caldav.png"), "CalDAV"),
            Backend::O365 => (resource_path!("icons/o365.png"), "O365"),
        };
        let image = gtk::Image::from_resource(icon);
        image.set_pixel_size(32);
        image.set_tooltip_text(Some(locale.translate(backend)));
        title.append(&image);

        let name = text(&model.id);
        name.add_css_class("ev_management_collection_name");
        title.append(&name);
        self.description.append(&title);

        if let Some(account) = &model.account {
            self.description.append(&text(account));
        }

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        actions.set_halign(gtk::Align::Start);

        let delete = icon_action("user-trash-symbolic", "Delete", locale);
        delete.add_css_class("ev_button_critical");
        actions.append(&delete);
        actions.append(&icon_action("pencil-symbolic", "Edit", locale));

        if model.reload {
            let reload = action("view-refresh-symbolic", "Reload", locale);
            reload.add_css_class("ev_button_warn");
            actions.append(&reload);
        }
        if model.discover {
            actions.append(&action("find-location-symbolic", "Discover", locale));
        }

        actions.append(&action("view-refresh-symbolic", "Sync", locale));
        if model.backend != Backend::FileSystem {
            actions.append(&action("view-list-symbolic", "Log", locale));
        }

        self.description.append(&actions);
    }
}

fn calendar_card(
    calendar: &Calendar,
    collection: &Collection,
    locale: &dyn Locale,
    toggle_group: &gtk::SizeGroup,
) -> gtk::Frame {
    let builder = gtk::Builder::from_resource(resource_path!("view/calendars/calendar.ui"));
    let widget: gtk::Frame = builder.object("calendar").unwrap();
    let heading: gtk::Box = builder.object("heading").unwrap();

    // Keep the color and name columns aligned even for calendars without an enablement control.
    let toggle_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    toggle_slot.set_valign(gtk::Align::Center);
    toggle_group.add_widget(&toggle_slot);
    heading.append(&toggle_slot);
    if calendar.configured {
        let enabled = gtk::Switch::builder().active(calendar.enabled).build();
        enabled.set_valign(gtk::Align::Center);
        enabled.set_tooltip_text(Some(locale.translate("Enabled")));
        enabled.update_property(&[gtk::accessible::Property::Label(
            locale.translate("Enabled"),
        )]);
        toggle_slot.append(&enabled);
    }

    let dot = gtk::DrawingArea::new();
    dot.set_size_request(22, 22);
    dot.set_valign(gtk::Align::Center);
    let color = calendar
        .background
        .parse::<gdk::RGBA>()
        .unwrap_or(gdk::RGBA::BLACK);
    dot.set_draw_func(move |_, context, width, height| {
        context.set_source_rgba(
            color.red().into(),
            color.green().into(),
            color.blue().into(),
            1.0,
        );
        context.arc(
            f64::from(width) / 2.0,
            f64::from(height) / 2.0,
            f64::from(width.min(height)) / 2.0 - 1.0,
            0.0,
            std::f64::consts::TAU,
        );
        let _ = context.fill();
    });
    heading.append(&dot);

    let name = text(&calendar.name);
    name.set_wrap(false);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.set_width_chars(1);
    name.set_tooltip_text(Some(&calendar.name));
    name.set_hexpand(true);
    heading.append(&name);

    let actions: gtk::Box = builder.object("actions").unwrap();
    if calendar.configured {
        let reload = action("view-refresh-symbolic", "Reload", locale);
        let delete = icon_action("user-trash-symbolic", "Delete", locale);
        if collection.backend != Backend::FileSystem {
            reload.add_css_class("ev_button_warn");
            delete.add_css_class("ev_button_warn");
        }
        actions.append(&reload);
        actions.append(&delete);
    } else if collection.create_calendar && collection.backend != Backend::FileSystem {
        let delete = icon_action("user-trash-symbolic", "Delete", locale);
        delete.add_css_class("ev_button_critical");
        actions.append(&delete);
    }

    let (icon, label) = if calendar.configured {
        ("pencil-symbolic", "Edit")
    } else {
        ("list-add-symbolic", "Add")
    };
    actions.append(&icon_action(icon, label, locale));

    let folder = icon_action("folder-symbolic", "Folder", locale);
    folder.set_tooltip_text(Some(&calendar.path));
    actions.append(&folder);

    widget
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

/// Placeholder controls intentionally have neither actions nor signal handlers.
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
    fn calendars_management_renders_unwired_actions_and_preserves_cards_and_scroll() {
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

        let card = view.rows.borrow()[0].cards[1].1.clone();
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

        let content = card.child().unwrap().downcast::<gtk::Box>().unwrap();
        assert_eq!(content.orientation(), gtk::Orientation::Horizontal);
        let heading = content.first_child().unwrap();
        let actions = content.last_child().unwrap();
        let heading_bounds = heading.compute_bounds(&card).unwrap();
        let action_bounds = actions.compute_bounds(&card).unwrap();
        assert!(heading_bounds.x() + heading_bounds.width() <= action_bounds.x());
        assert!((heading_bounds.center().y() - action_bounds.center().y()).abs() <= 1.0);
        assert!(card.height() <= 44, "calendars stay compact single rows");

        let unknown = view.rows.borrow()[0].cards[0].1.clone();
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
        assert_eq!(view.rows.borrow()[0].cards[1].1, card);
        assert_eq!(scroll.vadjustment().value(), 100.0);
        assert_eq!(
            view.actions.compute_bounds(&view.widget).unwrap(),
            footer_bounds
        );

        let mut updated = snapshot();
        updated.collections[0].calendars[0].name = "Renamed".into();
        view.set_snapshot(updated);
        drain();
        assert_eq!(view.rows.borrow()[0].cards[1].1, card);
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
            color_x(&view.rows.borrow()[0].cards[0].1),
            color_x(&view.rows.borrow()[1].cards[1].1)
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
}
