// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, rc::Rc, sync::Arc};

use eventix_ical::objects::CalPartStat;
use eventix_locale::Locale;
use eventix_state::actions::items::{ItemId, OccurrenceAction, TaskAction};
use eventix_view::item_actions::Action;
use eventix_view::{
    OccurrenceId,
    details::{DetailActions, Details, Response},
};
use gtk::{gio, glib, prelude::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuAction {
    Task(TaskAction),
    Respond(CalPartStat),
    Delete,
    Edit,
    Details,
    CopyUid,
    CopyFilepath,
    EditOccurrence,
    EditFollowing,
    RespondSeries(CalPartStat),
    Occurrence(OccurrenceAction),
}

/// An occurrence target, retaining its recurrence identity independently of displayed segments.
#[derive(Clone)]
pub(crate) struct OccurrenceTarget {
    pub item: ItemId,
    pub summary: String,
    pub recurrent: bool,
    pub task: bool,
}

impl OccurrenceTarget {
    pub fn new(id: &OccurrenceId, summary: &str, recurrent: bool, task: bool) -> Option<Self> {
        Some(Self {
            item: ItemId {
                calendar: id.calendar.clone(),
                uid: id.uid.clone(),
                recurrence: if id.recurrence.is_empty() {
                    None
                } else {
                    Some(id.recurrence.parse().ok()?)
                },
            },
            summary: summary.into(),
            recurrent,
            task,
        })
    }
}

type ContextSnapshot = (
    OccurrenceTarget,
    Arc<Details>,
    Arc<dyn Locale + Send + Sync>,
);

/// Installs a lazy context menu; the snapshot is read again for each right click.
pub(super) fn install_context(
    anchor: &impl IsA<gtk::Widget>,
    snapshot: impl Fn() -> Option<ContextSnapshot> + 'static,
) {
    for controller in anchor.observe_controllers().snapshot() {
        if let Ok(click) = controller.downcast::<gtk::GestureClick>()
            && click.button() == gtk::gdk::BUTTON_SECONDARY
        {
            anchor.remove_controller(&click);
        }
    }
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_SECONDARY);
    let weak = anchor.as_ref().downgrade();
    click.connect_pressed(move |gesture, _, x, y| {
        let Some(anchor) = weak.upgrade() else { return };
        let Some((target, details, locale)) = snapshot() else {
            return;
        };
        let Some(page) = anchor
            .ancestor(super::page::CalendarPage::static_type())
            .and_downcast::<super::page::CalendarPage>()
        else {
            return;
        };
        if page.editor_open() {
            return;
        }
        page.dismiss_popovers();
        let (model, group) = occurrence_menu(&details.actions, target.recurrent, &*locale);
        let handlers = Rc::new(Actions::default());
        let page = page.downgrade();
        handlers.connect(move |target, action| {
            if let Some(page) = page.upgrade() {
                page.occurrence_menu_action(target, action);
            }
        });
        bind(&group, &target, &handlers);
        let popup = popover(model.upcast_ref(), None);
        popup.insert_action_group("item", Some(&group));
        popup.set_has_arrow(false);
        popup.set_parent(&anchor);
        popup.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popup.connect_closed(|popup| popup.unparent());
        // Removing or recycling the source must also release its native popup.
        let weak = popup.downgrade();
        let unmap = anchor.connect_unmap(move |_| {
            if let Some(popup) = weak.upgrade() {
                popup.popdown();
            }
        });
        let weak = anchor.downgrade();
        let unmap = RefCell::new(Some(unmap));
        popup.connect_destroy(move |_| {
            if let Some(anchor) = weak.upgrade()
                && let Some(unmap) = unmap.take()
            {
                anchor.disconnect(unmap);
            }
        });
        popup.popup();
        gesture.set_state(gtk::EventSequenceState::Claimed);
    });
    anchor.add_controller(click);
}

fn occurrence_menu(
    available: &DetailActions,
    recurrent: bool,
    locale: &dyn Locale,
) -> (gio::Menu, gio::SimpleActionGroup) {
    let mut items = Vec::new();
    if available.complete {
        items.push(Action::Complete);
    }
    if available.postpone {
        items.push(Action::Postpone);
    }
    if available.delete {
        items.push(Action::Delete);
    }
    if available.edit {
        items.push(Action::Edit);
    }
    if let Some(response) = available.occurrence_response.or(available.series_response)
        && (!available.read_only || response != Response::Pending)
    {
        let status = match response {
            Response::Accepted => CalPartStat::Accepted,
            Response::Tentative => CalPartStat::Tentative,
            Response::Declined => CalPartStat::Declined,
            Response::Pending => CalPartStat::NeedsAction,
        };
        items.push(Action::Respond {
            status,
            read_only: available.read_only,
        });
    }
    items.push(Action::CopyUid);
    scoped_menu(&items, locale, recurrent, Some(available))
}

type Handler<T> = Box<dyn Fn(&T, MenuAction)>;

pub(super) struct Actions<T>(RefCell<Option<Handler<T>>>);

impl<T> Default for Actions<T> {
    fn default() -> Self {
        Self(RefCell::new(None))
    }
}

impl<T> Actions<T> {
    pub(super) fn connect(&self, handler: impl Fn(&T, MenuAction) + 'static) {
        self.0.replace(Some(Box::new(handler)));
    }
}

pub(super) fn button<T: Clone + 'static>(
    row: &T,
    available: &[Action],
    locale: &dyn Locale,
    anchor: &impl IsA<gtk::Widget>,
    handlers: &Rc<Actions<T>>,
) -> gtk::MenuButton {
    let (model, actions) = menu(available, locale);
    bind(&actions, row, handlers);
    let builder = gtk::Builder::from_resource(resource_path!("view/action_menu/button.ui"));
    let button: gtk::MenuButton = builder.object("button").unwrap();
    button.set_tooltip_text(Some(locale.translate("Actions")));
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

fn bind<T: Clone + 'static>(actions: &gio::SimpleActionGroup, row: &T, handlers: &Rc<Actions<T>>) {
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
        ("details", MenuAction::Details),
        ("copy-uid", MenuAction::CopyUid),
        ("copy-filepath", MenuAction::CopyFilepath),
        ("edit-occurrence", MenuAction::EditOccurrence),
        ("edit-following", MenuAction::EditFollowing),
        (
            "accept-series",
            MenuAction::RespondSeries(CalPartStat::Accepted),
        ),
        (
            "tentative-series",
            MenuAction::RespondSeries(CalPartStat::Tentative),
        ),
        (
            "decline-series",
            MenuAction::RespondSeries(CalPartStat::Declined),
        ),
        ("cancel", MenuAction::Occurrence(OccurrenceAction::Cancel)),
        ("exclude", MenuAction::Occurrence(OccurrenceAction::Exclude)),
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
    scoped_menu(available, locale, false, None)
}

fn scoped_menu(
    available: &[Action],
    locale: &dyn Locale,
    recurrent: bool,
    occurrence: Option<&DetailActions>,
) -> (gio::Menu, gio::SimpleActionGroup) {
    let model = gio::Menu::new();
    let actions = gio::SimpleActionGroup::new();
    let details = gio::Menu::new();
    let task = gio::Menu::new();
    let deletion = gio::Menu::new();
    let editing = gio::Menu::new();
    let invitation = gio::Menu::new();
    append(&details, &actions, locale, "Details", "details", true);
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
            Action::Delete => {}
            Action::Edit if recurrent => {
                let variants = gio::Menu::new();
                for (key, name) in [
                    ("This occurrence", "edit-occurrence"),
                    ("This and following occ.", "edit-following"),
                    ("Complete series", "edit"),
                ] {
                    append(&variants, &actions, locale, key, name, true);
                }
                append_submenu(&editing, locale, "Edit", &variants);
            }
            Action::Edit => append(&editing, &actions, locale, "Edit", "edit", true),
            Action::Alarms => {}
            Action::Respond { status, read_only } => append_response(
                &invitation,
                &actions,
                locale,
                *status,
                *read_only,
                recurrent.then_some("Occurrence"),
            ),
            Action::CopyUid => {
                append(&details, &actions, locale, "Copy UID", "copy-uid", true);
                append(
                    &details,
                    &actions,
                    locale,
                    "Copy Filepath",
                    "copy-filepath",
                    true,
                );
            }
        }
    }
    if let Some(occurrence) = occurrence {
        if occurrence.cancel {
            append(&deletion, &actions, locale, "Cancel occ.", "cancel", true);
        }
        if occurrence.exclude {
            append(&deletion, &actions, locale, "Exclude occ.", "exclude", true);
        }
        if recurrent
            && let Some(response) = occurrence.series_response
            && (!occurrence.read_only || response != Response::Pending)
        {
            let status = match response {
                Response::Accepted => CalPartStat::Accepted,
                Response::Tentative => CalPartStat::Tentative,
                Response::Declined => CalPartStat::Declined,
                Response::Pending => CalPartStat::NeedsAction,
            };
            append_response(
                &invitation,
                &actions,
                locale,
                status,
                occurrence.read_only,
                Some("Series"),
            );
        }
    }
    if available.contains(&Action::Delete) {
        append(
            &deletion,
            &actions,
            locale,
            if recurrent { "Delete series" } else { "Delete" },
            "delete",
            true,
        );
    }
    for index in 0..editing.n_items() {
        task.append_item(&gio::MenuItem::from_model(&editing, index));
    }
    for section in [details, invitation, task, deletion] {
        if section.n_items() > 0 {
            model.append_section(None, &section);
        }
    }
    (model, actions)
}

fn append_response(
    menu: &gio::Menu,
    actions: &gio::SimpleActionGroup,
    locale: &dyn Locale,
    status: CalPartStat,
    read_only: bool,
    scope: Option<&str>,
) {
    let key = match status {
        CalPartStat::Accepted => "Accepted",
        CalPartStat::Tentative => "Tentative",
        CalPartStat::Declined => "Declined",
        _ => "Respond",
    };
    let label = match (key, scope) {
        ("Respond", Some("Occurrence")) => locale.translate("Respond (Occurrence)").into(),
        ("Respond", Some("Series")) => locale.translate("Respond (Series)").into(),
        (_, Some(scope)) => format!("{} ({})", locale.translate(key), locale.translate(scope)),
        (_, None) => locale.translate(key).into(),
    };
    let item = menu_item(&label, key);
    let series = scope == Some("Series");
    if read_only {
        let name = if series {
            "response-series"
        } else {
            "response"
        };
        let action = gio::SimpleAction::new(name, None);
        action.set_enabled(false);
        actions.add_action(&action);
        item.set_detailed_action(&format!("item.{name}"));
    } else {
        let variants = gio::Menu::new();
        for (key, name) in [
            ("Accept", "accept"),
            ("Tentative", "tentative"),
            ("Decline", "decline"),
        ] {
            let name = if series {
                format!("{name}-series")
            } else {
                name.into()
            };
            append(&variants, actions, locale, key, &name, true);
        }
        item.set_submenu(Some(&variants));
    }
    menu.append_item(&item);
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
        "Delete" | "Delete series" | "Cancel occ." => ("trash", Some("error_color")),
        "Decline" | "Declined" => ("x-lg", Some("error_color")),
        "Tentative" => ("question-lg", Some("warning_color")),
        "Postpone" | "1day" | "2days" | "3days" | "5days" | "1week" => ("skip-forward-fill", None),
        "Edit" | "This occurrence" | "This and following occ." | "Complete series" => {
            ("pencil", None)
        }
        "Copy UID" => ("tag", None),
        "Copy Filepath" => ("journal-text", None),
        "Details" => ("card-heading", None),
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
    let builder = gtk::Builder::from_resource(resource_path!("view/action_menu/popover.ui"));
    let contents: gtk::Box = builder.object("contents").unwrap();
    let popover: gtk::Popover = builder.object("popover").unwrap();
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
            let builder =
                gtk::Builder::from_resource(resource_path!("view/action_menu/submenu.ui"));
            let button: gtk::MenuButton = builder.object("button").unwrap();
            button.set_child(Some(&row));
            button.set_popover(Some(&popover(&submenu, Some(root))));
            button.upcast()
        } else {
            let builder = gtk::Builder::from_resource(resource_path!("view/action_menu/item.ui"));
            let button: gtk::Button = builder.object("button").unwrap();
            button.set_child(Some(&row));
            if let Some(action) = string_attribute(&item, "action") {
                button.set_action_name(Some(&action));
                if action == "item.response" || action == "item.response-series" {
                    button.add_css_class("ev_action_response_status");
                }
            }
            let root = root.downgrade();
            let parent = parent.downgrade();
            // GTK's default clicked handler activates the action. Keep the popup and its
            // action group alive until that handler has run, including for native context menus.
            button.connect_closure(
                "clicked",
                true,
                gtk::glib::closure_local!(move |_button: gtk::Button| {
                    if let Some(parent) = parent.upgrade() {
                        parent.popdown();
                    }
                    if let Some(root) = root.upgrade() {
                        root.popdown();
                    }
                }),
            );
            button.upcast()
        };
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
    let builder = gtk::Builder::from_resource(resource_path!("view/action_menu/content.ui"));
    let row = builder.object("content").unwrap();
    let image: gtk::Image = builder.object("icon").unwrap();
    let icon = item
        .attribute_value("icon", None)
        .and_then(|value| gio::Icon::deserialize(&value));
    if let Some(icon) = &icon {
        image.set_from_gicon(icon);
    }
    image.set_visible(icon.is_some());
    let label: gtk::Label = builder.object("label").unwrap();
    label.set_text(
        string_attribute(item, "label")
            .as_deref()
            .unwrap_or_default(),
    );
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

    use super::{Actions, MenuAction, OccurrenceTarget, bind, menu, occurrence_menu};

    #[test]
    fn german_occurrence_menu_uses_full_response_labels_and_groups_destructive_actions() {
        use eventix_view::details::{DetailActions, Response};

        let tmp = tempfile::tempdir().unwrap();
        let xdg = xdg::BaseDirectories::with_prefix(tmp.path().file_name().unwrap());
        let locale = eventix_locale::new_with_timezone(
            &xdg,
            eventix_locale::LocaleType::German,
            chrono_tz::UTC,
        )
        .unwrap();
        let (model, _) = occurrence_menu(
            &DetailActions {
                delete: true,
                cancel: true,
                edit: true,
                occurrence_response: Some(Response::Tentative),
                series_response: Some(Response::Accepted),
                ..Default::default()
            },
            true,
            &*locale,
        );
        assert_eq!(model.n_items(), 4);
        let deletion = model.item_link(3, "section").unwrap();
        assert_eq!(deletion.n_items(), 2);
        assert_eq!(label(&deletion, 0), locale.translate("Cancel occ."));
        assert_eq!(label(&deletion, 1), locale.translate("Delete series"));
        let responses = model.item_link(1, "section").unwrap();
        assert_eq!(responses.n_items(), 2);
        let editing = model.item_link(2, "section").unwrap();
        assert_eq!(label(&editing, 0), locale.translate("Edit"));
        assert!(editing.item_link(0, "submenu").is_some());
        assert_eq!(label(&responses, 0), "Unverbindlich (Termin)");
        assert_eq!(label(&responses, 1), "Angenommen (Serie)");
        for index in 0..2 {
            let choices = responses.item_link(index, "submenu").unwrap();
            assert_eq!(label(&choices, 1), "Unverbindlich");
        }
        let (pending, _) = occurrence_menu(
            &DetailActions {
                occurrence_response: Some(Response::Pending),
                series_response: Some(Response::Pending),
                ..Default::default()
            },
            true,
            &*locale,
        );
        let responses = pending.item_link(1, "section").unwrap();
        assert_eq!(label(&responses, 0), "Antworten (Termin)");
        assert_eq!(label(&responses, 1), "Antworten (Serie)");
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn context_submenu_dispatches_selected_variant_before_closing() {
        use eventix_state::actions::items::ItemId;

        gtk::init().unwrap();
        gio::resources_register_include!("eventix.gresource").unwrap();
        let item = ItemId {
            calendar: "work".into(),
            uid: "task".into(),
            recurrence: Some("TU2026-10-25T09:00:00".parse().unwrap()),
        };
        let (model, group) = menu(&[Action::Postpone], &*eventix_locale::default());
        let recorded = Rc::new(RefCell::new(Vec::new()));
        let results = recorded.clone();
        let handlers = Rc::new(Actions::default());
        handlers.connect(move |item: &ItemId, command| {
            results.borrow_mut().push((item.clone(), command));
        });
        bind(&group, &item, &handlers);
        let anchor = gtk::Button::new();
        let window = gtk::Window::builder().child(&anchor).build();
        window.present();
        let drain = || {
            for _ in 0..20 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        drain();
        let popup = super::popover(model.upcast_ref(), None);
        popup.insert_action_group("item", Some(&group));
        popup.set_parent(&anchor);
        popup.connect_closed(|popup| popup.unparent());
        popup.popup();
        drain();
        let submenu = popup
            .child()
            .unwrap()
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap()
            .next_sibling()
            .unwrap()
            .downcast::<gtk::MenuButton>()
            .unwrap();
        submenu.popup();
        drain();
        let variants = submenu.popover().unwrap();
        let one_day = variants
            .child()
            .unwrap()
            .first_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        assert!(one_day.is_sensitive());
        one_day.emit_clicked();
        assert_eq!(
            recorded.borrow().as_slice(),
            &[(item, MenuAction::Task(TaskAction::Postpone { days: 1 }))]
        );
        drain();
        assert!(popup.parent().is_none());
        assert!(!variants.is_visible());
        window.close();
    }

    #[test]
    fn occurrence_commands_preserve_recurrence_and_offer_explicit_edit_scopes() {
        use eventix_view::{OccurrenceId, details::DetailActions};

        let id = OccurrenceId {
            calendar: "work".into(),
            uid: "shared-uid".into(),
            recurrence: "TU2026-10-25T09:00:00".into(),
        };
        let target = OccurrenceTarget::new(&id, "Recurring task", true, true).unwrap();
        let (model, group) = occurrence_menu(
            &DetailActions {
                edit: true,
                delete: true,
                complete: true,
                postpone: true,
                exclude: true,
                occurrence_response: Some(eventix_view::details::Response::Tentative),
                series_response: Some(eventix_view::details::Response::Accepted),
                ..Default::default()
            },
            true,
            &*eventix_locale::default(),
        );
        assert_eq!(model.n_items(), 4);
        let deletion = model.item_link(3, "section").unwrap();
        assert_eq!(deletion.n_items(), 2);
        assert_eq!(label(&deletion, 0), "Exclude occ.");
        assert_eq!(label(&deletion, 1), "Delete series");
        let editing = model.item_link(2, "section").unwrap();
        assert_eq!(editing.n_items(), 3);
        let scopes = editing.item_link(2, "submenu").unwrap();
        assert_eq!(label(&editing, 0), "Complete");
        assert_eq!(label(&editing, 1), "Postpone");
        assert_eq!(label(&editing, 2), "Edit");
        assert_eq!(label(&scopes, 0), "This occurrence");
        assert_eq!(label(&scopes, 1), "This and following occ.");
        assert_eq!(label(&scopes, 2), "Complete series");
        let invitation = model.item_link(1, "section").unwrap();
        assert_eq!(invitation.n_items(), 2);
        assert_eq!(label(&invitation, 0), "Tentative (Occurrence)");
        assert_eq!(label(&invitation, 1), "Accepted (Series)");

        let recorded = Rc::new(RefCell::new(Vec::new()));
        let results = recorded.clone();
        let handlers = Rc::new(Actions::default());
        handlers.connect(move |target: &OccurrenceTarget, command| {
            results.borrow_mut().push((target.item.clone(), command));
        });
        bind(&group, &target, &handlers);
        for name in [
            "edit-occurrence",
            "edit-following",
            "edit",
            "complete",
            "postpone-3",
            "exclude",
            "accept",
            "decline-series",
        ] {
            group.activate_action(name, None);
        }
        let commands = recorded.borrow();
        assert_eq!(commands.len(), 8);
        assert!(commands.iter().all(|(item, _)| item == &target.item));
        assert_eq!(commands[0].1, MenuAction::EditOccurrence);
        assert_eq!(commands[1].1, MenuAction::EditFollowing);
        assert_eq!(commands[2].1, MenuAction::Edit);
        assert_eq!(commands[6].1, MenuAction::Respond(CalPartStat::Accepted));
        assert_eq!(
            commands[7].1,
            MenuAction::RespondSeries(CalPartStat::Declined)
        );

        let (_, read_only) = occurrence_menu(
            &DetailActions {
                read_only: true,
                occurrence_response: Some(eventix_view::details::Response::Accepted),
                series_response: Some(eventix_view::details::Response::Tentative),
                personal_reminders: true,
                ..Default::default()
            },
            true,
            &*eventix_locale::default(),
        );
        assert!(!read_only.is_action_enabled("response"));
        for name in [
            "accept",
            "accept-series",
            "edit",
            "edit-occurrence",
            "delete",
            "complete",
        ] {
            assert!(read_only.lookup_action(name).is_none(), "{name}");
        }
        assert!(read_only.lookup_action("alarms").is_none());
        assert!(read_only.is_action_enabled("details"));
        assert!(read_only.is_action_enabled("copy-uid"));
        assert!(read_only.is_action_enabled("copy-filepath"));
        let mut invalid = id;
        invalid.recurrence = "invalid".into();
        assert!(OccurrenceTarget::new(&invalid, "", true, false).is_none());
    }

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
        handlers.connect(move |row: &Row, action| {
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
            ("details", MenuAction::Details),
            ("copy-uid", MenuAction::CopyUid),
            ("copy-filepath", MenuAction::CopyFilepath),
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
        let details = model.item_link(0, "section").unwrap();
        assert_eq!(details.n_items(), 3);
        assert_eq!(label(&details, 0), "Details");
        assert_eq!(label(&details, 1), "Copy UID");
        assert_eq!(label(&details, 2), "Copy Filepath");
        let task = model.item_link(1, "section").unwrap();
        assert_eq!(task.n_items(), 3);
        assert_eq!(label(&task, 0), "Complete");
        assert_eq!(label(&task, 1), "Postpone");
        assert_eq!(label(&task, 2), "Edit");
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
        let deletion = model.item_link(2, "section").unwrap();
        assert_eq!(deletion.n_items(), 1);
        assert_eq!(label(&deletion, 0), "Delete");
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
            let invitation = model.item_link(1, "section").unwrap();
            assert_eq!(label(&invitation, 0), "Accepted");
            assert_eq!(invitation.n_items(), 1);
            assert!(actions.lookup_action("alarms").is_none());
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
