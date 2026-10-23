// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::LazyLock;

use eventix_ical::objects::CalPartStat;
use eventix_locale::Locale;
use eventix_state::actions::items::{EditScope, ItemId, OccurrenceAction, TaskAction};
use eventix_view::{
    Event,
    details::{Attendee, Contact, Details, Response},
    sidebar::SidebarRow,
};
use gtk::{glib, prelude::*, subclass::prelude::*};
use regex::Regex;

use crate::style;

use super::indicators::EventIndicators;

mod arrow;
pub(crate) mod delete;

struct DetailItem<'a> {
    summary: &'a str,
    calendar: &'a str,
    background: &'a str,
    foreground: &'a str,
    details: &'a Details,
    recurrent: bool,
    exception: bool,
    alarm: bool,
}

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/hrniels/Eventix/gtk/view/details/details.ui")]
    pub struct EventDetails {
        pub edits: std::cell::RefCell<Vec<(gtk::Button, EditScope)>>,
        pub responses: std::cell::RefCell<Vec<(gtk::Button, CalPartStat, bool)>>,
        pub delete: std::cell::RefCell<Option<gtk::Button>>,
        pub occurrence: std::cell::RefCell<Option<(gtk::Button, OccurrenceAction)>>,
        pub tasks: std::cell::RefCell<Vec<(gtk::Button, TaskAction)>>,
        pub item: std::cell::RefCell<Option<ItemId>>,
        pub recurrent: std::cell::Cell<bool>,
        #[template_child]
        pub summary: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub tag: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub calendar: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub indicators: gtk::TemplateChild<EventIndicators>,
        #[template_child]
        pub rows: gtk::TemplateChild<gtk::Grid>,
        #[template_child]
        pub actions: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub actions_left: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub actions_right: gtk::TemplateChild<gtk::Box>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EventDetails {
        const NAME: &'static str = "EventixEventDetails";
        type Type = super::EventDetails;
        type ParentType = gtk::Popover;

        fn class_init(klass: &mut Self::Class) {
            EventIndicators::static_type();
            klass.bind_template();
        }
        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for EventDetails {
        fn dispose(&self) {
            self.obj().set_child(None::<&gtk::Widget>);
            self.dispose_template();
        }
    }
    impl WidgetImpl for EventDetails {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let Some(contents) = obj.first_child().filter(|_| obj.has_arrow()) else {
                self.parent_snapshot(snapshot);
                return;
            };
            super::arrow::snapshot(&obj, &contents, snapshot);
            obj.snapshot_child(&contents, snapshot);
        }
    }
    impl PopoverImpl for EventDetails {}
}

glib::wrapper! {
    pub struct EventDetails(ObjectSubclass<imp::EventDetails>)
        @extends gtk::Popover, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget,
            gtk::Native, gtk::ShortcutManager;
}

impl EventDetails {
    /// Measures the natural width and minimum height needed for anchored placement.
    pub(crate) fn placement_size(&self) -> (i32, i32) {
        // GtkWidget::measure returns zero for a hidden popover. Calling the parent's measure
        // vfunc lets us determine placement before showing it for the first time.
        let imp = self.imp();
        let (minimum_width, natural_width, _, _) =
            imp.parent_measure(gtk::Orientation::Horizontal, -1);
        let minimum_height = imp
            .parent_measure(gtk::Orientation::Vertical, minimum_width)
            .0
            .max(
                imp.parent_measure(gtk::Orientation::Vertical, natural_width)
                    .0,
            );
        (natural_width, minimum_height)
    }

    pub fn new(event: &Event, locale: &dyn Locale) -> Self {
        let obj = Self::from_item(
            &event.id.uid,
            &DetailItem {
                summary: &event.summary,
                calendar: &event.calendar,
                background: &event.background,
                foreground: &event.foreground,
                details: &event.details,
                recurrent: event.recurrent,
                exception: event.exception,
                alarm: event.alarm,
            },
            locale,
        );
        obj.set_identity(&event.id);
        obj
    }

    pub fn new_sidebar(row: &SidebarRow, locale: &dyn Locale) -> Self {
        let obj = Self::from_item(
            &row.id.uid,
            &DetailItem {
                summary: &row.summary,
                calendar: &row.calendar,
                background: &row.background,
                foreground: &row.foreground,
                details: &row.details,
                recurrent: row.recurrent,
                exception: row.exception,
                alarm: row.alarm,
            },
            locale,
        );
        obj.set_identity(&row.id);
        obj
    }

    fn set_identity(&self, id: &eventix_view::OccurrenceId) {
        self.imp().item.replace(Some(ItemId {
            calendar: id.calendar.clone(),
            uid: id.uid.clone(),
            recurrence: if id.recurrence.is_empty() {
                None
            } else {
                id.recurrence.parse().ok()
            },
        }));
    }

    pub fn connect_edit(&self, callback: impl Fn(ItemId, EditScope) + 'static) {
        let callback = std::rc::Rc::new(callback);
        for (button, scope) in self.imp().edits.borrow().iter() {
            let item = self.imp().item.borrow().clone();
            let callback = callback.clone();
            let scope = *scope;
            button.connect_clicked(move |_| {
                if let Some(item) = &item {
                    callback(item.clone(), scope);
                }
            });
        }
    }

    pub fn connect_delete(&self, callback: impl Fn(ItemId, bool, String) + 'static) {
        if let Some(button) = self.imp().delete.borrow().as_ref() {
            let item = self.imp().item.borrow().clone();
            let recurrent = self.imp().recurrent.get();
            let summary = self.imp().summary.text().to_string();
            button.connect_clicked(move |_| {
                if let Some(item) = &item {
                    callback(
                        ItemId {
                            recurrence: None,
                            ..item.clone()
                        },
                        recurrent,
                        summary.clone(),
                    );
                }
            });
        }
    }

    pub fn connect_occurrence(&self, callback: impl Fn(ItemId, OccurrenceAction) + 'static) {
        if let Some((button, action)) = self.imp().occurrence.borrow().as_ref() {
            let item = self.imp().item.borrow().clone();
            let action = *action;
            button.connect_clicked(move |_| {
                if let Some(item) = &item {
                    callback(item.clone(), action);
                }
            });
        }
    }

    pub fn connect_task(&self, callback: impl Fn(ItemId, TaskAction) + 'static) {
        let callback = std::rc::Rc::new(callback);
        for (button, action) in self.imp().tasks.borrow().iter() {
            let item = self.imp().item.borrow().clone();
            let callback = callback.clone();
            let action = *action;
            button.connect_clicked(move |_| {
                if let Some(item) = &item {
                    callback(item.clone(), action);
                }
            });
        }
    }

    pub fn connect_respond(&self, callback: impl Fn(ItemId, CalPartStat) + 'static) {
        let callback = std::rc::Rc::new(callback);
        for (button, response, occurrence) in self.imp().responses.borrow().iter() {
            let mut item = self.imp().item.borrow().clone();
            if !occurrence && let Some(item) = &mut item {
                item.recurrence = None;
            }
            let callback = callback.clone();
            let response = *response;
            button.connect_clicked(move |_| {
                if let Some(item) = &item {
                    callback(item.clone(), response);
                }
            });
        }
    }

    fn from_item(uid: &str, event: &DetailItem<'_>, locale: &dyn Locale) -> Self {
        let obj: Self = glib::Object::new();
        if let Some(child) = obj.child() {
            child.set_size_request(super::ITEM_PANEL_WIDTH, -1);
        }
        obj.setup_header(event, locale);
        obj.setup_tag(uid);
        obj.add_rows(event, locale);
        obj.add_actions(event, locale);
        obj.imp().recurrent.set(event.recurrent);
        obj
    }

    fn setup_header(&self, event: &DetailItem<'_>, locale: &dyn Locale) {
        let imp = self.imp();
        let summary = if event.summary.is_empty() {
            locale.translate("Untitled event")
        } else {
            event.summary
        };
        imp.summary.set_text(summary);
        imp.summary.set_tooltip_text(Some(summary));
        imp.indicators
            .set_state(event.recurrent, event.exception, event.alarm, locale);
        imp.calendar.set_tooltip_text(Some(event.calendar));
        imp.calendar
            .update_property(&[gtk::accessible::Property::Label(event.calendar)]);
        imp.calendar
            .add_css_class(&style::calendar_class(event.background, event.foreground));
        self.update_property(&[gtk::accessible::Property::Label(summary)]);
    }

    fn setup_tag(&self, uid: &str) {
        let imp = self.imp();
        imp.tag.set_tooltip_text(Some(uid));
        imp.tag.set_cursor_from_name(Some("pointer"));
        imp.tag
            .update_property(&[gtk::accessible::Property::Label("UID")]);
        let uid = uid.to_owned();
        imp.tag.connect_clicked(move |button| {
            button.clipboard().set_text(&uid);
        });

        let motion = gtk::EventControllerMotion::new();

        let tag_image = imp.tag.child().and_downcast::<gtk::Image>().unwrap();
        let image = tag_image.downgrade();
        motion.connect_enter(move |_, _, _| {
            if let Some(image) = image.upgrade() {
                image.set_resource(Some(
                    "/io/github/hrniels/Eventix/gtk/icons/tag-symbolic-hover.svg",
                ));
            }
        });

        let image = tag_image.downgrade();
        motion.connect_leave(move |_| {
            if let Some(image) = image.upgrade() {
                image.set_resource(Some(
                    "/io/github/hrniels/Eventix/gtk/icons/tag-symbolic.svg",
                ));
            }
        });

        imp.tag.add_controller(motion);
    }

    fn add_rows(&self, event: &DetailItem<'_>, locale: &dyn Locale) {
        let imp = self.imp();
        let details = &event.details;

        let scrollers =
            gtk::Builder::from_resource("/io/github/hrniels/Eventix/gtk/view/details/scrollers.ui");
        let attendees_scroll: gtk::ScrolledWindow = scrollers
            .object("attendees_scroll")
            .expect("attendee detail scroller is defined in GTK resources");
        let description_scroll: gtk::ScrolledWindow = scrollers
            .object("description_scroll")
            .expect("description detail scroller is defined in GTK resources");

        let titles = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
        let mut index = 0;
        let mut row = |icon_name: &str, key: &str, value: &gtk::Widget| {
            let row = detail_row(&titles, icon_name, locale.translate(key), value);
            imp.rows.attach(&row, 0, index, 1, 1);
            index += 1;
        };

        let date = if let Some(source) = &details.source_date {
            format!("{}\n{source}", details.date)
        } else {
            details.date.clone()
        };
        row(
            "calendar-date",
            details.date_label,
            label(&date).upcast_ref(),
        );

        if !details.location.trim().is_empty() {
            row(
                "geo-alt-fill",
                "Location",
                linked_label(&details.location).upcast_ref(),
            );
        }

        if let Some(status) = &details.status {
            row("flag", "Status", label(status).upcast_ref());
        }

        if let Some(recurrence) = &details.recurrence {
            row(
                "arrow-clockwise",
                "Repeats_short",
                label(recurrence).upcast_ref(),
            );
        }

        if let Some(reminders) = &details.reminders {
            row(
                "alarm",
                "Reminder",
                reminder_content(reminders, event, locale).upcast_ref(),
            );
        }

        if let Some(organizer) = &details.organizer {
            row(
                "person-fill",
                "Organizer",
                organizer_label(organizer, details.sent_by.as_deref(), locale).upcast_ref(),
            );
        }

        if !details.attendees.is_empty() {
            attendees_scroll.set_child(Some(&attendees_content(&details.attendees)));
            row("people-fill", "Attendees", attendees_scroll.upcast_ref());
        }

        if !details.description.trim().is_empty() {
            description_scroll.set_child(Some(&linked_label(details.description.trim())));
            row(
                "journal-text",
                "Description",
                description_scroll.upcast_ref(),
            );
        }
    }

    fn add_actions(&self, event: &DetailItem<'_>, locale: &dyn Locale) {
        let actions = &event.details.actions;
        let left = &self.imp().actions_left;
        let right = &self.imp().actions_right;

        if actions.delete {
            let delete = button(
                "trash",
                locale.translate(if event.recurrent {
                    "Delete series"
                } else {
                    "Delete"
                }),
                "ev_details_critical",
            );
            left.append(&delete);
            self.imp().delete.replace(Some(delete));
        }

        if actions.cancel || actions.exclude {
            let occurrence = button(
                "dash-circle",
                locale.translate(if actions.cancel {
                    "Cancel occ."
                } else {
                    "Exclude occ."
                }),
                if actions.cancel {
                    "ev_details_critical"
                } else {
                    ""
                },
            );
            left.append(&occurrence);
            self.imp().occurrence.replace(Some((
                occurrence,
                if actions.cancel {
                    OccurrenceAction::Cancel
                } else {
                    OccurrenceAction::Exclude
                },
            )));
        }

        if actions.complete {
            let complete = button("check-lg", locale.translate("Complete"), "ev_details_good");
            right.append(&complete);
            self.imp()
                .tasks
                .borrow_mut()
                .push((complete, TaskAction::Complete));
        }

        if actions.postpone {
            let choices = [
                ("1day", 1),
                ("2days", 2),
                ("3days", 3),
                ("5days", 5),
                ("1week", 7),
            ];
            let items: Vec<_> = choices
                .iter()
                .map(|(key, _)| ("", locale.translate(key), ""))
                .collect();
            let (postpone, options) = menu(
                "skip-forward-fill",
                locale.translate("Postpone"),
                "",
                &items,
            );
            for (button, (_, days)) in options.into_iter().zip(choices) {
                self.imp()
                    .tasks
                    .borrow_mut()
                    .push((button, TaskAction::Postpone { days }));
            }
            right.append(&postpone);
        }

        if actions.edit {
            if event.recurrent {
                let (edit_menu, options) = menu(
                    "pencil",
                    locale.translate("Edit"),
                    "",
                    &[
                        ("calendar-event", locale.translate("This occurrence"), ""),
                        (
                            "calendar-range",
                            locale.translate("This and following occ."),
                            "",
                        ),
                        ("arrow-clockwise", locale.translate("Complete series"), ""),
                    ],
                );
                for (button, scope) in options.into_iter().zip([
                    EditScope::Occurrence,
                    EditScope::Following,
                    EditScope::Series,
                ]) {
                    self.imp().edits.borrow_mut().push((button, scope));
                }
                right.append(&edit_menu);
            } else {
                let edit = button("pencil", locale.translate("Edit"), "");
                self.imp()
                    .edits
                    .borrow_mut()
                    .push((edit.clone(), EditScope::Series));
                right.append(&edit);
            }
        }

        for (response, occurrence) in [
            (actions.series_response, false),
            (actions.occurrence_response, true),
        ] {
            let Some(response) = response else { continue };
            let (icon, key, class) = match response {
                Response::Accepted => ("check-lg", "Accepted", "ev_details_good"),
                Response::Declined => ("x-lg", "Declined", "ev_details_critical"),
                Response::Tentative => ("question-lg", "Tentative", "ev_details_warn"),
                Response::Pending => (
                    "reply",
                    if event.recurrent {
                        if occurrence { "Occurrence" } else { "Series" }
                    } else {
                        "Respond"
                    },
                    "",
                ),
            };

            if actions.read_only && response == Response::Pending {
                continue;
            }

            let text = if event.recurrent && response != Response::Pending {
                format!(
                    "{} {}",
                    locale.translate(key),
                    locale.translate(if occurrence { "(Occ.)" } else { "(Ser.)" })
                )
            } else {
                locale.translate(key).to_owned()
            };

            if actions.read_only {
                let status = button_content(icon, &text);
                status.add_css_class(class);
                right.append(&status);
            } else {
                let (response_menu, options) = menu(
                    icon,
                    &text,
                    class,
                    &[
                        ("check-lg", locale.translate("Accept"), "ev_details_good"),
                        (
                            "question-lg",
                            locale.translate("Tentative"),
                            "ev_details_warn",
                        ),
                        ("x-lg", locale.translate("Decline"), "ev_details_critical"),
                    ],
                );
                for (button, response) in options.into_iter().zip([
                    CalPartStat::Accepted,
                    CalPartStat::Tentative,
                    CalPartStat::Declined,
                ]) {
                    self.imp()
                        .responses
                        .borrow_mut()
                        .push((button, response, occurrence));
                }
                right.append(&response_menu);
            }
        }

        let has_left = left.first_child().is_some();
        let has_right = right.first_child().is_some();
        left.set_visible(has_left);
        right.set_visible(has_right);
        self.imp().actions.set_visible(has_left || has_right);
    }
}

fn detail_row(
    titles: &gtk::SizeGroup,
    icon_name: &str,
    title: &str,
    value: &gtk::Widget,
) -> gtk::Grid {
    let row = gtk::Grid::builder()
        .column_spacing(12)
        .hexpand(true)
        .build();
    row.add_css_class("ev_details_row");
    let image = icon(icon_name);
    image.set_valign(gtk::Align::Start);
    let title = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .valign(gtk::Align::Start)
        .build();
    title.add_css_class("ev_details_label");
    titles.add_widget(&title);
    value.update_relation(&[gtk::accessible::Relation::LabelledBy(&[title.upcast_ref()])]);
    row.attach(&image, 0, 0, 1, 1);
    row.attach(&title, 1, 0, 1, 1);
    row.attach(value, 2, 0, 1, 1);
    row
}

fn reminder_content(reminders: &str, event: &DetailItem<'_>, locale: &dyn Locale) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.append(&label(reminders));
    if event.details.actions.personal_reminders {
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        for key in if event.recurrent {
            &["Edit series", "Edit occ."][..]
        } else {
            &["Edit"][..]
        } {
            buttons.append(&button("pencil", locale.translate(key), ""));
        }
        content.append(&buttons);
    }
    content
}

fn organizer_label(organizer: &Contact, sent_by: Option<&str>, locale: &dyn Locale) -> gtk::Label {
    let text = label("");
    let mut markup = contact_markup(organizer);
    if let Some(sent_by) = sent_by {
        markup.push_str(&format!(
            ", {} {}",
            glib::markup_escape_text(locale.translate("sent by")),
            link_markup(sent_by, sent_by)
        ));
    }
    text.set_markup(&markup);
    text
}

fn attendees_content(attendees: &[Attendee]) -> gtk::Box {
    let people = gtk::Box::new(gtk::Orientation::Vertical, 6);
    for attendee in attendees {
        let person = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let suffix = match attendee.response {
            Response::Accepted => "-check",
            Response::Declined => "-slash",
            Response::Tentative => "-exclamation",
            Response::Pending => "",
        };
        let image = icon(&format!(
            "person{}{suffix}",
            if attendee.required { "-fill" } else { "" }
        ));
        image.set_valign(gtk::Align::Start);
        image.set_tooltip_text(Some(&attendee.tooltip));
        image.update_property(&[gtk::accessible::Property::Label(&attendee.tooltip)]);
        person.append(&image);
        let name = label("");
        name.set_markup(&contact_markup(&attendee.contact));
        name.set_tooltip_text(Some(&format!(
            "{}\n{}",
            attendee.contact.address, attendee.tooltip
        )));
        person.append(&name);
        people.append(&person);
    }
    people
}

fn icon(name: &str) -> gtk::Image {
    gtk::Image::builder()
        .resource(format!(
            "/io/github/hrniels/Eventix/gtk/icons/{name}-symbolic.svg"
        ))
        .pixel_size(16)
        .build()
}

fn label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .valign(gtk::Align::Start)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .selectable(true)
        .hexpand(true)
        .max_width_chars(42)
        .build()
}

fn button_content(icon_name: &str, text: &str) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.set_halign(gtk::Align::Center);
    if !icon_name.is_empty() {
        content.append(&icon(icon_name));
    }
    content.append(&gtk::Label::new(Some(text)));
    content
}

fn button(icon_name: &str, text: &str, class: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .child(&button_content(icon_name, text))
        .halign(gtk::Align::Start)
        .build();
    button.add_css_class("ev_details_button");
    if !class.is_empty() {
        button.add_css_class(class);
    }
    button.update_property(&[gtk::accessible::Property::Label(text)]);
    button
}

/// Builds a consistently styled dropdown with caller-defined action options.
fn menu(
    icon_name: &str,
    text: &str,
    class: &str,
    items: &[(&str, &str, &str)],
) -> (gtk::MenuButton, Vec<gtk::Button>) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let popover = gtk::Popover::builder()
        .child(&content)
        .has_arrow(false)
        .position(gtk::PositionType::Bottom)
        .halign(gtk::Align::Start)
        .build();
    popover.add_css_class("ev_details_menu");
    let mut options = Vec::with_capacity(items.len());
    for (icon, title, class) in items {
        let item = button(icon, title, class);
        item.set_halign(gtk::Align::Fill);
        if let Some(content) = item.child() {
            content.set_halign(gtk::Align::Start);
        }
        let weak = popover.downgrade();
        item.connect_clicked(move |_| {
            if let Some(popover) = weak.upgrade() {
                popover.popdown();
            }
        });
        content.append(&item);
        options.push(item);
    }
    let menu = gtk::MenuButton::builder()
        .child(&button_content(icon_name, text))
        .popover(&popover)
        .halign(gtk::Align::Start)
        .build();
    menu.add_css_class("ev_details_button");
    if !class.is_empty() {
        menu.add_css_class(class);
    }
    menu.update_property(&[gtk::accessible::Property::Label(text)]);
    let weak = menu.downgrade();
    popover.connect_show(move |popover| {
        if let Some(menu) = weak.upgrade() {
            popover.set_width_request(menu.width());
        }
    });
    (menu, options)
}

fn contact_markup(contact: &Contact) -> String {
    link_markup(&format!("mailto:{}", contact.address), &contact.name)
}

fn link_markup(uri: &str, text: &str) -> String {
    format!(
        "<a href=\"{}\">{}</a>",
        glib::markup_escape_text(uri),
        glib::markup_escape_text(text)
    )
}

fn linked_label(text: &str) -> gtk::Label {
    let label = label("");
    label.set_markup(&text_markup(text));
    label
}

/// Escapes calendar text before adding links, preserving literal markup and line breaks.
fn text_markup(text: &str) -> String {
    static LINKS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?:https?://|mailto:)?(?:www\.)?[-a-zA-Z0-9@:%._+~#=]{1,256}",
            r"\.[a-zA-Z0-9]{2,}\b(?:[-a-zA-Z0-9()@:;%_+.~#?&/=]*)"
        ))
        .expect("valid link regex")
    });
    let mut result = String::new();
    let mut end = 0;
    for found in LINKS.find_iter(text) {
        result.push_str(&glib::markup_escape_text(&text[end..found.start()]));
        let text = found.as_str().trim_end_matches(['.', ',', ';', ')']);
        let uri = if text.starts_with("https:")
            || text.starts_with("http:")
            || text.starts_with("mailto:")
        {
            text.to_owned()
        } else if text.contains('@') {
            format!("mailto:{text}")
        } else {
            format!("https://{text}")
        };
        result.push_str(&link_markup(&uri, text));
        end = found.start() + text.len();
    }
    result.push_str(&glib::markup_escape_text(&text[end..]));
    result
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::TimeZone;
    use eventix_view::{
        AllDaySpan, Event, OccurrenceId, TimedEvent,
        details::{Attendee, Contact, DetailActions, Details, Response},
    };
    use gtk::{prelude::*, subclass::prelude::*};

    use super::text_markup;

    #[test]
    #[ignore = "requires a GTK display"]
    fn occurrence_buttons_forward_the_selected_identity_and_action() {
        use eventix_state::actions::items::{OccurrenceAction, TaskAction};

        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let locale = eventix_locale::default();
        let id = OccurrenceId {
            calendar: "cal".into(),
            uid: "meeting".into(),
            recurrence: "TU2026-10-13T09:00:00".into(),
        };
        for action in [
            None,
            Some(OccurrenceAction::Exclude),
            Some(OccurrenceAction::Cancel),
        ] {
            let details = Details {
                date_label: "Date",
                date: String::new(),
                source_date: None,
                location: String::new(),
                description: String::new(),
                status: None,
                recurrence: None,
                reminders: None,
                organizer: None,
                sent_by: None,
                attendees: vec![],
                actions: DetailActions {
                    exclude: action == Some(OccurrenceAction::Exclude),
                    cancel: action == Some(OccurrenceAction::Cancel),
                    complete: true,
                    postpone: true,
                    ..Default::default()
                },
            };
            let popup = super::EventDetails::from_item(
                &id.uid,
                &super::DetailItem {
                    summary: "Meeting",
                    calendar: "Test",
                    background: "#336699",
                    foreground: "#ffffff",
                    details: &details,
                    recurrent: true,
                    exception: action.is_some(),
                    alarm: action.is_some(),
                },
                locale.as_ref(),
            );
            let indicators = &popup.imp().indicators;
            assert!(indicators.property::<bool>("visible"));
            assert_eq!(
                indicators.next_sibling(),
                Some(popup.imp().calendar.clone().upcast())
            );
            let titles = if action.is_some() {
                vec!["Recurring event", "Recurrence exception", "Reminder"]
            } else {
                vec!["Recurring event"]
            };
            let images: Vec<gtk::Image> = indicators
                .observe_children()
                .snapshot()
                .into_iter()
                .map(|child| child.downcast::<gtk::Image>().unwrap())
                .filter(|image| image.property::<bool>("visible"))
                .collect();
            assert_eq!(images.len(), titles.len());
            for (image, title) in images.iter().zip(titles) {
                assert_eq!(
                    image.tooltip_text().as_deref(),
                    Some(locale.translate(title))
                );
            }
            popup.set_identity(&id);
            let tasks = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let result = tasks.clone();
            popup.connect_task(move |item, action| result.borrow_mut().push((item, action)));
            for (button, _) in popup.imp().tasks.borrow().iter() {
                button.emit_clicked();
            }
            let tasks = tasks.borrow();
            let expected = [
                TaskAction::Complete,
                TaskAction::Postpone { days: 1 },
                TaskAction::Postpone { days: 2 },
                TaskAction::Postpone { days: 3 },
                TaskAction::Postpone { days: 5 },
                TaskAction::Postpone { days: 7 },
            ];
            assert_eq!(tasks.len(), expected.len());
            for ((item, action), expected) in tasks.iter().zip(expected) {
                assert_eq!(item.calendar, id.calendar);
                assert_eq!(item.uid, id.uid);
                assert_eq!(item.recurrence.as_ref().unwrap().to_string(), id.recurrence);
                assert_eq!(*action, expected);
            }
            let received = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let result = received.clone();
            popup.connect_occurrence(move |item, action| result.borrow_mut().push((item, action)));
            if let Some((button, _)) = popup.imp().occurrence.borrow().as_ref() {
                button.emit_clicked();
            }
            let received = received.borrow();
            if let Some(action) = action {
                assert_eq!(received.len(), 1);
                assert_eq!(received[0].0.calendar, id.calendar);
                assert_eq!(received[0].0.uid, id.uid);
                assert_eq!(
                    received[0].0.recurrence.as_ref().unwrap().to_string(),
                    id.recurrence
                );
                assert_eq!(received[0].1, action);
            } else {
                assert!(received.is_empty());
            }
        }
    }

    /// Exercises template loading, layout, symbolic resources and nested menus on a real display.
    #[test]
    #[ignore = "requires a GTK display"]
    fn popup_layout_and_dummy_menus() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let display = gtk::gdk::Display::default().unwrap();
        crate::style::install(&display);
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        for language in [
            eventix_locale::LocaleType::English,
            eventix_locale::LocaleType::German,
        ] {
            let locale = eventix_locale::new_with_timezone(
                &xdg,
                language,
                *eventix_locale::default().timezone(),
            )
            .unwrap();
            for variant in ["full", "sparse", "invitation", "read_only"] {
                let start = locale
                    .timezone()
                    .with_ymd_and_hms(2026, 10, 5, 9, 0, 0)
                    .unwrap();
                let full = variant != "sparse";
                let read_only = variant == "read_only";
                let invitation = variant == "invitation" || read_only;
                let contact = Contact {
                    name: "Amy & Zoe <Team>".into(),
                    address: "amy@example.org".into(),
                };
                let event = Event {
                    id: OccurrenceId {
                        calendar: "cal".into(),
                        uid: "popup-test".into(),
                        recurrence: String::new(),
                    },
                    summary: if full {
                        "Planning <&> review — a long title which should wrap neatly".into()
                    } else {
                        "Short event".into()
                    },
                    calendar: "Work".into(),
                    foreground: "#ffffff".into(),
                    background: "#336699".into(),
                    start,
                    end: start + chrono::Duration::hours(1),
                    all_day: false,
                    cancelled: false,
                    recurrent: full,
                    exception: false,
                    alarm: full,
                    details: Arc::new(Details {
                        date_label: "Date",
                        date: "Oct 05, 2026, 09:00 ‒ 10:00".into(),
                        source_date: full
                            .then(|| "Oct 05, 2026, 11:00 ‒ 12:00 (Europe/Berlin)".into()),
                        location: if full {
                            "Meeting room\nhttps://example.org/?a=1&b=2".into()
                        } else {
                            String::new()
                        },
                        description: if full {
                            "Long description with literal <markup> & links: https://example.org\n"
                                .repeat(20)
                        } else {
                            String::new()
                        },
                        status: full.then(|| locale.translate("CONFIRMED").into()),
                        recurrence: full.then(|| locale.translate("Daily").to_owned()),
                        reminders: full.then(|| "15 minutes before start".into()),
                        organizer: full.then(|| contact.clone()),
                        sent_by: full.then(|| "mailto:assistant@example.org".into()),
                        attendees: if full {
                            (0..20)
                                .map(|i| Attendee {
                                    contact: contact.clone(),
                                    required: i % 2 == 0,
                                    response: [
                                        Response::Accepted,
                                        Response::Declined,
                                        Response::Tentative,
                                        Response::Pending,
                                    ][i % 4],
                                    tooltip: "Required, Accepted".into(),
                                })
                                .collect()
                        } else {
                            vec![]
                        },
                        actions: DetailActions {
                            delete: !read_only,
                            edit: !invitation,
                            cancel: full && !invitation,
                            exclude: false,
                            series_response: invitation.then_some(Response::Accepted),
                            occurrence_response: invitation.then_some(Response::Tentative),
                            read_only,
                            personal_reminders: full,
                            ..Default::default()
                        },
                    }),
                };
                let mut response_event = event.clone();
                response_event.id.recurrence = "TU2026-10-05T09:00:00".into();
                let response_details = super::EventDetails::new(&response_event, locale.as_ref());
                let responses = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                let received = responses.clone();
                response_details.connect_respond(move |item, response| {
                    received.borrow_mut().push((item, response));
                });
                for (button, _, _) in response_details.imp().responses.borrow().iter() {
                    button.emit_clicked();
                }
                let responses = responses.borrow();
                if invitation && !read_only {
                    assert_eq!(responses.len(), 6);
                    for (index, (item, response)) in responses.iter().enumerate() {
                        assert_eq!(item.calendar, "cal");
                        assert_eq!(item.uid, "popup-test");
                        assert_eq!(item.recurrence.is_some(), index >= 3);
                        assert_eq!(
                            *response,
                            [
                                eventix_ical::objects::CalPartStat::Accepted,
                                eventix_ical::objects::CalPartStat::Tentative,
                                eventix_ical::objects::CalPartStat::Declined,
                            ][index % 3]
                        );
                    }
                } else {
                    assert!(responses.is_empty());
                }
                let colors = gtk::CssProvider::new();
                let week = std::rc::Rc::new(eventix_view::WeekSnapshot {
                    start: start.date_naive(),
                    locale: locale.clone(),
                    days: vec![eventix_view::Day {
                        date: start.date_naive(),
                        timed: if full {
                            vec![TimedEvent {
                                event: event.clone(),
                                date: start.date_naive(),
                                minute_start: 540,
                                minute_end: 600,
                                columns: eventix_view::layout::Columns {
                                    count: 1,
                                    offset: 0,
                                    span: 1,
                                },
                            }]
                        } else {
                            vec![]
                        },
                        dst: vec![],
                    }],
                    all_day: if full {
                        vec![]
                    } else {
                        vec![AllDaySpan {
                            event: event.clone(),
                            start_day: 0,
                            end_day: 1,
                            row: 0,
                            continues_before: false,
                            continues_after: false,
                        }]
                    },
                });
                colors.load_from_data(&crate::style::calendar_css(&week));
                gtk::style_context_add_provider_for_display(
                    &display,
                    &colors,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
                let details = std::rc::Rc::new(crate::view::popover::DetailsController::default());
                let (anchor, cards): (gtk::Widget, _) = if full {
                    let grid: crate::view::weekly::timed_grid::TimedGrid = gtk::glib::Object::new();
                    grid.set_details(details.clone());
                    grid.set_week(week.clone());
                    let cards = grid.event_cards();
                    (grid.upcast(), cards)
                } else {
                    let strip: crate::view::weekly::all_day_strip::AllDayStrip =
                        gtk::glib::Object::new();
                    strip.set_details(details.clone());
                    strip.set_week(week.clone());
                    let cards = strip.event_cards();
                    (strip.upcast(), cards)
                };
                let scroll = gtk::ScrolledWindow::builder()
                    .hscrollbar_policy(gtk::PolicyType::Never)
                    .child(&anchor)
                    .build();
                let outer_scroll = gtk::ScrolledWindow::builder()
                    .vscrollbar_policy(gtk::PolicyType::Never)
                    .child(&scroll)
                    .build();
                let window = gtk::Window::builder()
                    .default_width(if full { 520 } else { 900 })
                    .default_height(if full { 430 } else { 750 })
                    .child(&outer_scroll)
                    .build();
                window.present();
                let drain = || {
                    for _ in 0..20 {
                        while gtk::glib::MainContext::default().iteration(false) {}
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                };
                drain();
                if full {
                    scroll.vadjustment().set_value(400.0);
                    drain();
                }
                cards[0].1.emit_clicked();
                drain();
                let mut child = anchor.first_child();
                let popup = loop {
                    let widget = child.expect("event surface details popup exists");
                    if let Some(popup) = widget.downcast_ref::<super::EventDetails>() {
                        break popup.clone();
                    }
                    child = widget.next_sibling();
                };
                assert!(
                    popup.width() > 400 && popup.width() < 850,
                    "width: {}",
                    popup.width()
                );
                assert!(popup.is_mapped(), "details popup did not map for {variant}");
                assert!(popup.height() < 1200, "height: {}", popup.height());
                assert_eq!(popup.imp().rows.child_at(0, 1).is_some(), full);
                if let Some(directory) = std::env::var_os("EVENTIX_DETAILS_SCREENSHOTS") {
                    let paintable = gtk::WidgetPaintable::new(Some(&popup));
                    let snapshot = gtk::Snapshot::new();
                    paintable.snapshot(&snapshot, popup.width().into(), popup.height().into());
                    let node = snapshot.to_node().unwrap();
                    let texture = popup.renderer().unwrap().render_texture(&node, None);
                    texture
                        .save_to_png(
                            std::path::PathBuf::from(directory)
                                .join(format!("{language:?}-{variant}.png")),
                        )
                        .unwrap();
                }
                popup.imp().tag.emit_clicked();
                assert_eq!(popup.imp().actions_left.first_child().is_some(), !read_only);
                assert_eq!(
                    popup.imp().actions_right.first_child().is_some(),
                    !invitation || full
                );
                for group in [&popup.imp().actions_left, &popup.imp().actions_right] {
                    let mut action = group.first_child();
                    while let Some(widget) = action {
                        let next = widget.next_sibling();
                        if let Some(menu) = widget.downcast_ref::<gtk::MenuButton>() {
                            menu.popup();
                            drain();
                            let dropdown = menu.popover().unwrap();
                            if !dropdown.is_visible() {
                                action = next;
                                continue;
                            }
                            let first = dropdown
                                .child()
                                .unwrap()
                                .first_child()
                                .unwrap()
                                .downcast::<gtk::Button>()
                                .unwrap();
                            first.emit_clicked();
                            assert!(!dropdown.is_visible());
                        } else if let Some(button) = widget.downcast_ref::<gtk::Button>() {
                            button.emit_clicked();
                            assert!(popup.is_visible());
                        }
                        action = next;
                    }
                }
                if variant == "full" && language == eventix_locale::LocaleType::English {
                    window.set_default_size(900, 1000);
                    scroll.vadjustment().set_value(500.0);
                    drain();
                    assert!(popup.is_mapped());
                    assert!(
                        popup.has_arrow(),
                        "roomy windows should retain event anchoring"
                    );
                    let (_, minimum_height) = popup.placement_size();
                    assert!(minimum_height > 0);
                    window.set_default_size(520, 430);
                    assert_eq!(popup.position(), gtk::PositionType::Bottom);
                    assert_eq!(popup.offset(), (0, 0));
                    drain();
                    let popup_center = || {
                        let surface = popup.surface().unwrap();
                        let position = surface.downcast_ref::<gtk::gdk::Popup>().unwrap();
                        position.position_y() + surface.height() / 2
                    };
                    let center = popup_center();
                    let popup_x = || {
                        popup
                            .surface()
                            .unwrap()
                            .downcast::<gtk::gdk::Popup>()
                            .unwrap()
                            .position_x()
                    };
                    let x = popup_x();
                    for value in [0, 100, 250, 275, 300, 350, 400, 450, 500, 550, 700, 900] {
                        scroll.vadjustment().set_value(f64::from(value));
                        drain();
                        assert!(
                            popup.is_mapped(),
                            "popup disappeared at scroll offset {value}"
                        );
                        let current_center = popup_center();
                        assert_eq!(
                            popup_x(),
                            x,
                            "horizontal position changed at scroll offset {value}"
                        );
                        assert!(
                            (current_center - center).abs() < 2,
                            "centered popup moved vertically at scroll offset {value}: {} -> {}, position {:?}",
                            center,
                            current_center,
                            popup.position()
                        );
                    }
                    assert_eq!(popup.position(), gtk::PositionType::Right);
                    assert!(!popup.has_arrow());
                    outer_scroll.hadjustment().set_value(300.0);
                    drain();
                    assert!(popup.is_mapped());
                    assert!(!popup.has_arrow());
                    assert!((popup_center() - center).abs() < 2);
                    outer_scroll.hadjustment().set_value(0.0);
                    scroll.vadjustment().set_value(500.0);
                    drain();
                    let bounds = cards[0].1.compute_bounds(&window).unwrap();
                    let event_bottom = (bounds.y() + bounds.height()).ceil() as i32;
                    for extra in [20, 40, 60, 40, 20] {
                        window.set_default_size(900, minimum_height + event_bottom + extra);
                        popup.queue_resize();
                        drain();
                        assert!(popup.is_mapped());
                        assert!(
                            !popup.has_arrow(),
                            "centered placement flickered near threshold"
                        );
                    }
                    window.set_default_size(900, minimum_height + event_bottom + 100);
                    drain();
                    assert!(popup.is_mapped());
                    assert!(popup.has_arrow(), "placement did not leave centered mode");
                    for extra in [79, 40, 20] {
                        window.set_default_size(900, minimum_height + event_bottom + extra);
                        popup.queue_resize();
                        drain();
                        assert!(popup.is_mapped());
                        assert!(
                            popup.has_arrow(),
                            "anchored placement flickered near threshold"
                        );
                    }
                    window.set_default_size(900, minimum_height + event_bottom + 100);
                    drain();
                    scroll
                        .vadjustment()
                        .set_value(scroll.vadjustment().upper() - scroll.vadjustment().page_size());
                    drain();
                    assert!(popup.is_mapped());
                    assert!(!popup.has_arrow(), "off-screen event retained its anchor");
                    scroll.vadjustment().set_value(500.0);
                    drain();
                    assert!(popup.is_mapped());
                    assert!(popup.has_arrow(), "visible event did not regain its anchor");
                    // GTK's native layout rejects allocations smaller than the minimum,
                    // even when the window-relative placement estimate predicted enough room.
                    let surface = popup.surface().unwrap();
                    surface.emit_by_name::<()>("layout", &[&surface.width(), &1i32]);
                    drain();
                    assert!(
                        popup.is_mapped(),
                        "rejected placement was not recovered: visible={}, parented={}",
                        popup.is_visible(),
                        popup.parent().is_some()
                    );
                    assert!(
                        !popup.has_arrow(),
                        "rejected placement did not use centered fallback"
                    );
                    window.set_default_size(900, 1000);
                    drain();
                    assert!(popup.is_mapped());
                    assert!(
                        !popup.has_arrow(),
                        "rejected placement retried its original mode"
                    );
                    window.set_default_size(520, 430);
                    drain();
                    assert!(popup.is_mapped());
                    assert!(!popup.has_arrow());
                }
                popup.popdown();
                assert!(
                    popup.parent().is_some(),
                    "cleanup must wait for GTK dismissal"
                );
                drain();
                assert!(popup.parent().is_none());
                let weak_popup = popup.downgrade();
                drop(popup);
                assert!(weak_popup.upgrade().is_none());
                if variant == "full" && language == eventix_locale::LocaleType::English {
                    let mut centered_y = None;
                    for value in [0, 100, 250, 350, 450, 550, 700, 900] {
                        scroll.vadjustment().set_value(f64::from(value));
                        drain();
                        cards[0].1.emit_clicked();
                        if value == 350 {
                            let mut child = anchor.first_child();
                            while let Some(widget) = child {
                                if let Some(popup) = widget.downcast_ref::<super::EventDetails>() {
                                    let surface = popup.surface().unwrap();
                                    surface
                                        .emit_by_name::<()>("layout", &[&surface.width(), &1i32]);
                                }
                                child = widget.next_sibling();
                            }
                        }
                        drain();
                        let mut child = anchor.first_child();
                        let mut mapped = false;
                        while let Some(widget) = child {
                            if widget.is::<super::EventDetails>() {
                                mapped |= widget.is_mapped();
                                let popup = widget.downcast_ref::<super::EventDetails>().unwrap();
                                assert!(!popup.has_arrow());
                                let surface = popup.surface().unwrap();
                                let position = surface.downcast_ref::<gtk::gdk::Popup>().unwrap();
                                let y = position.position_y() + surface.height() / 2;
                                let expected = *centered_y.get_or_insert(y);
                                assert!(
                                    (y - expected).abs() < 2,
                                    "off-screen popup was not centered at scroll offset {value}"
                                );
                            }
                            child = widget.next_sibling();
                        }
                        assert!(mapped, "popup did not open at scroll offset {value}");
                        details.close();
                        drain();
                    }
                }
                cards[0].1.emit_clicked();
                drain();
                details.close();
                drain();
                window.close();
                drain();
                gtk::style_context_remove_provider_for_display(&display, &colors);
                if variant == "sparse" {
                    crate::view::page::tests::assert_sidebar_layout_and_details(
                        eventix_view::WeekSnapshot {
                            start: week.start,
                            days: week.days.clone(),
                            all_day: week.all_day.clone(),
                            locale: week.locale.clone(),
                        },
                    );
                    crate::view::page::tests::assert_month_layout_and_navigation(
                        eventix_view::WeekSnapshot {
                            start: week.start,
                            days: week.days.clone(),
                            all_day: week.all_day.clone(),
                            locale: week.locale.clone(),
                        },
                    );
                }
                if variant == "sparse" && language == eventix_locale::LocaleType::English {
                    crate::view::page::tests::assert_span_layout_and_refresh(
                        eventix_view::WeekSnapshot {
                            start: week.start,
                            days: week.days.clone(),
                            all_day: week.all_day.clone(),
                            locale: week.locale.clone(),
                        },
                    );
                }
            }
        }
    }

    #[test]
    fn calendar_text_is_escaped_before_linking() {
        let markup = text_markup("<b>Meeting</b>\nhttps://example.org/?a=1&b=2. me@example.org");
        assert_eq!(
            markup,
            concat!(
                "&lt;b&gt;Meeting&lt;/b&gt;\n",
                "<a href=\"https://example.org/?a=1&amp;b=2\">",
                "https://example.org/?a=1&amp;b=2</a>. ",
                "<a href=\"mailto:me@example.org\">me@example.org</a>"
            )
        );
    }
}
