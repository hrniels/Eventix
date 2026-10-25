// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, rc::Rc};

use eventix_ical::objects::CalPartStat;
use eventix_locale::Locale;
use eventix_state::actions::items::TaskAction;
use eventix_view::list::{Action, Row};
use gtk::{gio, prelude::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuAction {
    Task(TaskAction),
    Respond(CalPartStat),
    Delete,
    Edit,
    Alarms,
    CopyUid,
}

type Handler = Box<dyn Fn(&Row, MenuAction)>;

#[derive(Default)]
pub(super) struct Actions(RefCell<Option<Handler>>);

impl Actions {
    pub(super) fn connect(&self, handler: impl Fn(&Row, MenuAction) + 'static) {
        self.0.replace(Some(Box::new(handler)));
    }
}

pub(super) fn button(
    row: &Row,
    locale: &dyn Locale,
    anchor: &gtk::Box,
    handlers: &Rc<Actions>,
) -> gtk::MenuButton {
    let (model, actions) = menu(&row.actions, locale);
    bind(&actions, row, handlers);
    let button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(locale.translate("Actions"))
        .valign(gtk::Align::Center)
        .hexpand(false)
        .build();
    button.add_css_class("ev_list_menu");
    button.update_property(&[gtk::accessible::Property::Label(
        locale.translate("Actions"),
    )]);
    button.insert_action_group("item", Some(&actions));
    // Virtualized rows only need the popover's widgets once their menu is opened.
    button.set_create_popup_func(move |button| {
        let popover = popover(model.upcast_ref(), None);
        popover.set_halign(gtk::Align::End);
        button.set_popover(Some(&popover));
    });

    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_SECONDARY);
    let weak = button.downgrade();
    click.connect_pressed(move |gesture, _, _, _| {
        if let Some(button) = weak.upgrade() {
            button.popup();
            gesture.set_state(gtk::EventSequenceState::Claimed);
        }
    });
    anchor.add_controller(click);
    button
}

fn bind(actions: &gio::SimpleActionGroup, row: &Row, handlers: &Rc<Actions>) {
    let row = Rc::new(row.clone());
    for (name, command) in [
        ("complete", MenuAction::Task(TaskAction::Complete)),
        (
            "postpone-1",
            MenuAction::Task(TaskAction::Postpone { days: 1 }),
        ),
        (
            "postpone-2",
            MenuAction::Task(TaskAction::Postpone { days: 2 }),
        ),
        (
            "postpone-3",
            MenuAction::Task(TaskAction::Postpone { days: 3 }),
        ),
        (
            "postpone-5",
            MenuAction::Task(TaskAction::Postpone { days: 5 }),
        ),
        (
            "postpone-7",
            MenuAction::Task(TaskAction::Postpone { days: 7 }),
        ),
        ("accept", MenuAction::Respond(CalPartStat::Accepted)),
        ("tentative", MenuAction::Respond(CalPartStat::Tentative)),
        ("decline", MenuAction::Respond(CalPartStat::Declined)),
        ("delete", MenuAction::Delete),
        ("edit", MenuAction::Edit),
        ("alarms", MenuAction::Alarms),
        ("copy-uid", MenuAction::CopyUid),
    ] {
        let Some(action) = actions
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
        else {
            continue;
        };
        let handlers = handlers.clone();
        let row = row.clone();
        action.connect_activate(move |_, _| {
            if let Some(handler) = handlers.0.borrow().as_ref() {
                handler(&row, command);
            }
        });
    }
}

fn menu(available: &[Action], locale: &dyn Locale) -> (gio::Menu, gio::SimpleActionGroup) {
    let model = gio::Menu::new();
    let actions = gio::SimpleActionGroup::new();
    let task = gio::Menu::new();
    let editing = gio::Menu::new();
    let invitation = gio::Menu::new();
    let utility = gio::Menu::new();
    for action in available {
        match action {
            Action::Complete => append(&task, &actions, locale, "Complete", "complete", true),
            Action::Postpone => {
                let variants = gio::Menu::new();
                for (key, days) in [
                    ("1day", 1),
                    ("2days", 2),
                    ("3days", 3),
                    ("5days", 5),
                    ("1week", 7),
                ] {
                    append(
                        &variants,
                        &actions,
                        locale,
                        key,
                        &format!("postpone-{days}"),
                        true,
                    );
                }
                append_submenu(&task, locale, "Postpone", &variants);
            }
            Action::Delete => append(&editing, &actions, locale, "Delete", "delete", true),
            Action::Edit => append(&editing, &actions, locale, "Edit", "edit", true),
            Action::Alarms => append(&invitation, &actions, locale, "Alarms", "alarms", true),
            Action::Respond { status, read_only } => {
                let key = match status {
                    CalPartStat::Accepted => "Accepted",
                    CalPartStat::Tentative => "Tentative",
                    CalPartStat::Declined => "Declined",
                    _ => "Respond",
                };
                if *read_only {
                    append(&invitation, &actions, locale, key, "response", false);
                } else {
                    let variants = gio::Menu::new();
                    for (key, name) in [
                        ("Accept", "accept"),
                        ("Tentative", "tentative"),
                        ("Decline", "decline"),
                    ] {
                        append(&variants, &actions, locale, key, name, true);
                    }
                    append_submenu(&invitation, locale, key, &variants);
                }
            }
            Action::CopyUid => append(&utility, &actions, locale, "Copy UID", "copy-uid", true),
        }
    }
    for section in [task, editing, invitation, utility] {
        if section.n_items() > 0 {
            model.append_section(None, &section);
        }
    }
    (model, actions)
}

fn append(
    menu: &gio::Menu,
    actions: &gio::SimpleActionGroup,
    locale: &dyn Locale,
    key: &str,
    name: &str,
    enabled: bool,
) {
    let action = gio::SimpleAction::new(name, None);
    action.set_enabled(enabled);
    actions.add_action(&action);
    let item = menu_item(locale.translate(key), key);
    item.set_detailed_action(&format!("item.{name}"));
    menu.append_item(&item);
}

fn append_submenu(menu: &gio::Menu, locale: &dyn Locale, key: &str, submenu: &gio::Menu) {
    let item = menu_item(locale.translate(key), key);
    item.set_submenu(Some(submenu));
    menu.append_item(&item);
}

fn menu_item(label: &str, key: &str) -> gio::MenuItem {
    let (icon, color) = match key {
        "Complete" | "Accept" | "Accepted" => ("check-lg", Some("success_color")),
        "Delete" => ("trash", Some("error_color")),
        "Decline" | "Declined" => ("x-lg", Some("error_color")),
        "Tentative" => ("question-lg", Some("warning_color")),
        "Postpone" | "1day" | "2days" | "3days" | "5days" | "1week" => ("skip-forward-fill", None),
        "Edit" | "Alarms" => ("pencil", None),
        "Copy UID" => ("tag", None),
        _ => ("reply", None),
    };
    let item = gio::MenuItem::new(Some(label), None);
    let file = gio::File::for_uri(&format!(
        "resource://{}/icons/{icon}-symbolic.svg",
        crate::RESOURCE_PREFIX
    ));
    item.set_icon(&gio::FileIcon::new(&file));
    if let Some(color) = color {
        item.set_attribute_value("ev-color", Some(&color.to_variant()));
    }
    item
}

/// Builds menu rows with both icons and labels, including nested variant menus.
fn popover(model: &gio::MenuModel, root: Option<&gtk::Popover>) -> gtk::Popover {
    let contents = gtk::Box::new(gtk::Orientation::Vertical, 0);
    contents.set_size_request(180, -1);
    let popover = gtk::Popover::builder()
        .child(&contents)
        .accessible_role(gtk::AccessibleRole::Menu)
        .build();
    popover.add_css_class("ev_list_action_menu");
    append_rows(&contents, model, &popover, root.unwrap_or(&popover));
    popover
}

fn append_rows(
    contents: &gtk::Box,
    model: &gio::MenuModel,
    parent: &gtk::Popover,
    root: &gtk::Popover,
) {
    for index in 0..model.n_items() {
        if let Some(section) = model.item_link(index, "section") {
            if contents.first_child().is_some() {
                contents.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            }
            append_rows(contents, &section, parent, root);
            continue;
        }
        let item = gio::MenuItem::from_model(model, index);
        let row = item_content(&item);
        let widget: gtk::Widget = if let Some(submenu) = model.item_link(index, "submenu") {
            gtk::MenuButton::builder()
                .child(&row)
                .direction(gtk::ArrowType::Right)
                .always_show_arrow(true)
                .popover(&popover(&submenu, Some(root)))
                .accessible_role(gtk::AccessibleRole::MenuItem)
                .build()
                .upcast()
        } else {
            let button = gtk::Button::builder()
                .child(&row)
                .accessible_role(gtk::AccessibleRole::MenuItem)
                .build();
            if let Some(action) = string_attribute(&item, "action") {
                button.set_action_name(Some(&action));
                if action == "item.response" {
                    button.add_css_class("ev_list_response_status");
                }
            }
            let root = root.downgrade();
            let parent = parent.downgrade();
            button.connect_clicked(move |_| {
                if let Some(parent) = parent.upgrade() {
                    parent.popdown();
                }
                if let Some(root) = root.upgrade() {
                    root.popdown();
                }
            });
            button.upcast()
        };
        widget.add_css_class("ev_list_menu_item");
        if let Some(color) = string_attribute(&item, "ev-color") {
            widget.add_css_class(match color.as_str() {
                "error_color" => "ev_details_critical",
                "success_color" => "ev_details_good",
                _ => "ev_details_warn",
            });
        }
        contents.append(&widget);
    }
}

fn item_content(item: &gio::MenuItem) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    if let Some(icon) = item
        .attribute_value("icon", None)
        .and_then(|value| gio::Icon::deserialize(&value))
    {
        let image = gtk::Image::from_gicon(&icon);
        image.set_pixel_size(16);
        row.append(&image);
    }
    let label = gtk::Label::new(string_attribute(item, "label").as_deref());
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);
    row
}

fn string_attribute(item: &gio::MenuItem, name: &str) -> Option<String> {
    item.attribute_value(name, None)
        .and_then(|value| value.get())
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use eventix_ical::objects::CalPartStat;
    use eventix_state::actions::items::TaskAction;
    use eventix_view::list::{Action, Row};
    use gtk::{gio, prelude::*};

    use super::{Actions, MenuAction, bind, menu};

    #[test]
    fn menu_dispatch_preserves_calendar_identity_and_each_selected_variant() {
        let row = Row {
            calendar: "work".into(),
            calendar_name: "Work".into(),
            uid: "shared-uid".into(),
            summary: "Task".into(),
            background: String::new(),
            task: true,
            completed: false,
            recurrent: false,
            alarm: false,
            response: None,
            date_range: String::new(),
            recurrence: String::new(),
            actions: vec![
                Action::Complete,
                Action::Postpone,
                Action::Edit,
                Action::Delete,
                Action::Respond {
                    status: CalPartStat::NeedsAction,
                    read_only: false,
                },
                Action::Alarms,
                Action::CopyUid,
            ],
        };
        let recorded = Rc::new(RefCell::new(Vec::new()));
        let results = recorded.clone();
        let handlers = Rc::new(Actions::default());
        handlers.connect(move |row, action| {
            results
                .borrow_mut()
                .push((row.calendar.clone(), row.uid.clone(), action));
        });
        let (_, group) = menu(&row.actions, &*eventix_locale::default());
        bind(&group, &row, &handlers);
        let expected = [
            ("complete", MenuAction::Task(TaskAction::Complete)),
            (
                "postpone-1",
                MenuAction::Task(TaskAction::Postpone { days: 1 }),
            ),
            (
                "postpone-2",
                MenuAction::Task(TaskAction::Postpone { days: 2 }),
            ),
            (
                "postpone-3",
                MenuAction::Task(TaskAction::Postpone { days: 3 }),
            ),
            (
                "postpone-5",
                MenuAction::Task(TaskAction::Postpone { days: 5 }),
            ),
            (
                "postpone-7",
                MenuAction::Task(TaskAction::Postpone { days: 7 }),
            ),
            ("accept", MenuAction::Respond(CalPartStat::Accepted)),
            ("tentative", MenuAction::Respond(CalPartStat::Tentative)),
            ("decline", MenuAction::Respond(CalPartStat::Declined)),
            ("edit", MenuAction::Edit),
            ("delete", MenuAction::Delete),
            ("alarms", MenuAction::Alarms),
            ("copy-uid", MenuAction::CopyUid),
        ];
        for (name, _) in expected {
            group.activate_action(name, None);
        }
        assert_eq!(
            *recorded.borrow(),
            expected.map(|(_, action)| ("work".into(), "shared-uid".into(), action))
        );
    }

    fn label(model: &gio::MenuModel, index: i32) -> String {
        model
            .item_attribute_value(index, "label", None)
            .unwrap()
            .get()
            .unwrap()
    }

    #[test]
    fn task_menu_groups_actions_and_offers_each_postponement_interval() {
        let (model, actions) = menu(
            &[
                Action::Complete,
                Action::Postpone,
                Action::Delete,
                Action::Edit,
                Action::CopyUid,
            ],
            &*eventix_locale::default(),
        );
        assert_eq!(model.n_items(), 3);
        let task = model.item_link(0, "section").unwrap();
        assert_eq!(label(&task, 0), "Complete");
        assert_eq!(label(&task, 1), "Postpone");
        let postpone = task.item_link(1, "submenu").unwrap();
        assert_eq!(postpone.n_items(), 5);
        for (index, (title, days)) in [
            ("1day", 1),
            ("2days", 2),
            ("3days", 3),
            ("5days", 5),
            ("1week", 7),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(label(&postpone, index as i32), title);
            assert!(actions.is_action_enabled(&format!("postpone-{days}")));
        }
        let editing = model.item_link(1, "section").unwrap();
        assert_eq!(label(&editing, 0), "Delete");
        assert_eq!(label(&editing, 1), "Edit");
        let utility = model.item_link(2, "section").unwrap();
        assert_eq!(label(&utility, 0), "Copy UID");
    }

    #[test]
    fn invitation_menu_offers_response_variants_unless_read_only() {
        for read_only in [false, true] {
            let (model, actions) = menu(
                &[
                    Action::Respond {
                        status: CalPartStat::Accepted,
                        read_only,
                    },
                    Action::Alarms,
                    Action::CopyUid,
                ],
                &*eventix_locale::default(),
            );
            assert_eq!(model.n_items(), 2);
            let invitation = model.item_link(0, "section").unwrap();
            assert_eq!(label(&invitation, 0), "Accepted");
            assert_eq!(label(&invitation, 1), "Alarms");
            if read_only {
                assert!(invitation.item_link(0, "submenu").is_none());
                assert!(!actions.is_action_enabled("response"));
            } else {
                let responses = invitation.item_link(0, "submenu").unwrap();
                assert_eq!(responses.n_items(), 3);
                for (index, title) in ["Accept", "Tentative", "Decline"].into_iter().enumerate() {
                    assert_eq!(label(&responses, index as i32), title);
                }
            }
        }
    }
}
