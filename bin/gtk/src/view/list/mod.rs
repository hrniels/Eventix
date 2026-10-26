// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod content;
mod filtering;
mod item;
mod model;
mod results;
mod row;
mod scroll;

use std::{cell::RefCell, rc::Rc};

use eventix_view::list::{Filter, Snapshot};
use gtk::prelude::*;

use crate::model::session::{
    ListRowsRequest, ListRowsResponse, OccurrencesRequest, OccurrencesResponse,
};

use self::{filtering::Filtering, results::Results};

use super::action_menu::MenuAction;

/// Coordinates independent filtering and virtualized results.
pub(crate) struct ListView {
    pub widget: gtk::Box,
    filtering: Rc<Filtering>,
    results: Results,
    applied: RefCell<Filter>,
}

impl ListView {
    pub fn new() -> Rc<Self> {
        let builder = gtk::Builder::from_resource(resource_path!("view/list/content.ui"));
        let widget: gtk::Box = builder.object("content").unwrap();
        let filtering = Filtering::new();
        let results = Results::new();
        filtering.update(&[], &Filter::default(), &*eventix_locale::default());
        widget.prepend(&filtering.widget);
        widget.append(&results.widget);
        let shortcuts = gtk::ShortcutController::new();
        shortcuts.set_scope(gtk::ShortcutScope::Global);
        let weak = Rc::downgrade(&filtering);
        shortcuts.add_shortcut(gtk::Shortcut::new(
            Some(gtk::KeyvalTrigger::new(
                gtk::gdk::Key::f,
                gtk::gdk::ModifierType::CONTROL_MASK,
            )),
            Some(gtk::CallbackAction::new(move |_, _| {
                if let Some(filtering) = weak.upgrade() {
                    filtering.focus_search();
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            })),
        ));
        widget.add_controller(shortcuts);
        let view = Rc::new(Self {
            widget,
            filtering,
            results,
            applied: RefCell::default(),
        });
        let weak = Rc::downgrade(&view);
        view.filtering.connect_focus_results(move || {
            weak.upgrade()
                .is_some_and(|view| view.results.focus_first())
        });
        view
    }

    pub fn filter(&self) -> Filter {
        self.applied.borrow().clone()
    }

    pub fn header(&self) -> &gtk::Box {
        &self.filtering.widget
    }

    pub fn connect_changed(self: &Rc<Self>, changed: impl Fn() + 'static) {
        let weak = Rc::downgrade(self);
        self.filtering.connect_changed(move || {
            if let Some(view) = weak.upgrade() {
                view.applied.replace(view.filtering.filter());
                view.results.reset_scroll();
                changed();
            }
        });
    }

    pub fn connect_load_rows(&self, load: impl Fn(ListRowsRequest) + 'static) {
        self.results.model.connect_load_rows(load);
    }

    pub fn connect_load_occurrences(&self, load: impl Fn(OccurrencesRequest) + 'static) {
        self.results.model.connect_load_occurrences(load);
    }

    pub fn connect_occurrence_action(
        &self,
        handler: impl Fn(&super::action_menu::OccurrenceTarget, MenuAction) + 'static,
    ) {
        self.results.occurrence_actions.connect(handler);
    }

    pub fn set_occurrences(&self, response: OccurrencesResponse) -> Result<(), String> {
        self.results.model.set_occurrences(response)
    }

    pub fn connect_action(&self, handler: impl Fn(&eventix_view::list::Row, MenuAction) + 'static) {
        self.results.actions.connect(handler);
    }

    pub fn set_snapshot(&self, snapshot: Snapshot) {
        self.filtering
            .update(&snapshot.calendars, &snapshot.filter, &*snapshot.locale);
        self.applied.replace(snapshot.filter.clone());
        self.results.update(snapshot);
    }

    pub fn set_rows(&self, response: ListRowsResponse) -> Result<(), String> {
        self.results.model.set_rows(response)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use eventix_state::actions::items::ItemId;
    use eventix_view::list::{Action, Calendar, Filter, Row, Snapshot};
    use gtk::prelude::*;

    use super::{ListView, MenuAction, item::ListItem, row::FixedRow};
    use crate::model::session::ListRowsResponse;

    fn wait_until(ready: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !ready() {
            assert!(
                std::time::Instant::now() < deadline,
                "list work did not finish"
            );
            while gtk::glib::MainContext::default().iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    fn descendants<T: IsA<gtk::Widget> + StaticType>(widget: &gtk::Widget) -> Vec<T> {
        let mut found = Vec::new();
        if let Ok(item) = widget.clone().downcast::<T>() {
            found.push(item);
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            found.extend(descendants::<T>(&widget));
            child = widget.next_sibling();
        }
        found
    }

    fn snapshot(filter: Filter) -> Snapshot {
        Snapshot {
            filter,
            locale: eventix_locale::default(),
            calendars: ["Work", "Home"]
                .map(|name| Calendar {
                    id: name.into(),
                    name: name.into(),
                    background: "#803366".into(),
                })
                .to_vec(),
            items: (0..5000)
                .map(|index| ItemId {
                    calendar: "Work".into(),
                    uid: format!("item-{index}"),
                    recurrence: None,
                })
                .collect(),
        }
    }

    fn row(item: &ItemId) -> Row {
        let recurrent = item
            .uid
            .strip_prefix("item-")
            .unwrap()
            .parse::<usize>()
            .unwrap()
            % 2
            == 0;
        let mut actions = Vec::new();
        if !recurrent {
            actions.extend([Action::Complete, Action::Postpone]);
        }
        actions.extend([Action::Delete, Action::Edit, Action::CopyUid]);
        Row {
            calendar: item.calendar.clone(),
            calendar_name: "Work".into(),
            uid: item.uid.clone(),
            summary: format!("Item {}", item.uid.strip_prefix("item-").unwrap()),
            background: "#803366".into(),
            task: true,
            completed: false,
            recurrent,
            alarm: true,
            response: None,
            date_range: "2026-10-25".into(),
            recurrence: "Every day, until the end of the year. ".repeat(8),
            actions,
        }
    }

    fn occurrence_page(
        request: &crate::model::session::OccurrencesRequest,
    ) -> eventix_view::list::occurrences::Page {
        use chrono::Duration;
        use eventix_ical::objects::{CalDate, CalDateType, DateContext};
        use eventix_view::{
            details::{DetailActions, Details, PersonalReminders},
            list::occurrences::{Cursor, Direction, Page, Row},
        };
        use std::sync::Arc;
        let rows = (0..request.count)
            .map(|index| {
                let offset = match request.direction {
                    Direction::Earlier => index as i64 - request.count as i64,
                    Direction::From => index as i64,
                    Direction::Later => index as i64 + 1,
                };
                let date = request.cursor.start.date_naive() + Duration::days(offset);
                let recurrence = CalDate::Date(date, CalDateType::Inclusive);
                let start = DateContext::system()
                    .date(&recurrence)
                    .start_in(&request.cursor.start.timezone());
                Arc::new(Row {
                    item: ItemId {
                        recurrence: Some(recurrence.clone()),
                        ..request.item.clone()
                    },
                    cursor: Cursor {
                        start,
                        recurrence: recurrence.to_string(),
                    },
                    summary: format!("Occurrence {}", start.date_naive()),
                    calendar_name: "Work".into(),
                    background: "#803366".into(),
                    response: Some(
                        [
                            eventix_view::details::Response::Accepted,
                            eventix_view::details::Response::Tentative,
                            eventix_view::details::Response::Declined,
                            eventix_view::details::Response::Pending,
                        ][index % 4],
                    ),
                    task: true,
                    completed: false,
                    cancelled: false,
                    excluded: false,
                    overwritten: true,
                    alarm: true,
                    details: Arc::new(Details {
                        date_label: "Date",
                        date: start.date_naive().to_string(),
                        source_date: None,
                        location: String::new(),
                        description: String::new(),
                        status: None,
                        recurrence: None,
                        reminders: None,
                        personal_reminders: PersonalReminders::default(),
                        organizer: None,
                        sent_by: None,
                        attendees: Vec::new(),
                        actions: DetailActions {
                            edit: true,
                            exclude: true,
                            complete: true,
                            ..Default::default()
                        },
                    }),
                })
            })
            .collect();
        Page { rows, more: true }
    }

    #[test]
    fn expansion_pages_refresh_preserve_identity_and_reject_collapsed_or_stale_responses() {
        use super::{item::Entry, model::ListModel};
        use crate::model::session::{OccurrencesRequest, OccurrencesResponse};
        use eventix_view::list::occurrences::Direction;
        let model: ListModel = gtk::glib::Object::new();
        let requests = Rc::new(RefCell::new(Vec::<OccurrencesRequest>::new()));
        let pending = requests.clone();
        model.connect_load_occurrences(move |request| pending.borrow_mut().push(request));
        let a = ItemId {
            calendar: "Work".into(),
            uid: "shared".into(),
            recurrence: None,
        };
        let b = ItemId {
            calendar: "Home".into(),
            ..a.clone()
        };
        model.set_items(vec![a.clone(), b.clone()]);
        model.toggle(&a, &chrono_tz::Europe::Berlin);
        assert!(model.is_expanded(&a));
        assert!(!model.is_expanded(&b));
        let initial = requests.take();
        assert_eq!(initial.len(), 2);
        assert_eq!(
            initial[0].cursor.start.timezone(),
            chrono_tz::Europe::Berlin
        );
        for request in &initial {
            model
                .set_occurrences(OccurrencesResponse {
                    request: request.clone(),
                    result: Ok(occurrence_page(request)),
                })
                .unwrap();
        }
        assert_eq!(model.n_items(), 14);
        assert_eq!(
            model.item(3),
            model.item(3),
            "live occurrence objects have stable identity"
        );
        model.load_more(&a, Direction::Earlier);
        let more = requests.take().pop().unwrap();
        model
            .set_occurrences(OccurrencesResponse {
                result: Ok(occurrence_page(&more)),
                request: more,
            })
            .unwrap();
        assert_eq!(model.n_items(), 19);
        let retained = model.item(3).unwrap().downcast::<ListItem>().unwrap();
        model.set_items(vec![a.clone(), b.clone()]);
        let refresh = requests.take();
        assert_eq!(refresh[0].count, 10);
        assert_eq!(refresh[1].count, 5);
        let mut refreshed = occurrence_page(&refresh[0]);
        for row in &mut refreshed.rows {
            std::sync::Arc::make_mut(row).summary.push_str(" refreshed");
        }
        model
            .set_occurrences(OccurrencesResponse {
                request: refresh[0].clone(),
                result: Ok(refreshed),
            })
            .unwrap();
        assert_eq!(
            model.item(3).unwrap(),
            retained.clone().upcast::<gtk::glib::Object>()
        );
        assert!(
            matches!(retained.entry().as_ref(), Some(Entry::Occurrence(row)) if row.summary.ends_with(" refreshed"))
        );
        let count = model.n_items();
        model
            .set_occurrences(OccurrencesResponse {
                request: initial[0].clone(),
                result: Err("obsolete failure".into()),
            })
            .unwrap();
        assert_eq!(model.n_items(), count);
        model.toggle(&a, &chrono_tz::Europe::Berlin);
        assert_eq!(model.n_items(), 2);
        model.toggle(&a, &chrono_tz::Europe::Berlin);
        model
            .set_occurrences(OccurrencesResponse {
                request: refresh[0].clone(),
                result: Err("collapsed request".into()),
            })
            .unwrap();
        for request in requests.take() {
            model
                .set_occurrences(OccurrencesResponse {
                    result: Ok(occurrence_page(&request)),
                    request,
                })
                .unwrap();
        }
        let rows: Vec<_> = (0..model.n_items())
            .filter_map(|index| {
                let item = model.item(index).unwrap().downcast::<ListItem>().unwrap();
                if let Some(Entry::Occurrence(row)) = item.entry().as_ref() {
                    Some(row.item.clone())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(rows.len(), 10);
        assert!(
            rows.iter()
                .all(|item| item.calendar == "Work" && item.recurrence.is_some())
        );
        model.set_items(vec![b]);
        assert!(!model.is_expanded(&a));
        assert_eq!(model.n_items(), 1);
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn keyboard_navigation_selects_scrolls_opens_menus_and_toggles_occurrences() {
        use gtk::{gdk, glib};

        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gdk::Display::default().unwrap());
        let view = ListView::new();
        let weak = Rc::downgrade(&view);
        view.connect_load_rows(move |request| {
            weak.upgrade()
                .unwrap()
                .set_rows(ListRowsResponse {
                    revision: request.revision,
                    result: Ok(request
                        .items
                        .into_iter()
                        .map(|id| {
                            let row = row(&id);
                            (id, Some(row))
                        })
                        .collect()),
                })
                .unwrap();
        });
        view.set_snapshot(snapshot(Filter::default()));
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(500)
            .child(&view.widget)
            .build();
        window.present();
        let list = descendants::<gtk::ListView>(view.widget.upcast_ref()).remove(0);
        let selection = list.model().and_downcast::<gtk::SingleSelection>().unwrap();
        let drain = || {
            for _ in 0..15 {
                while glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        // Activate GTK's actual key bindings as well as the list's capture controller.
        let press = |widget: &gtk::Widget, key: gdk::Key, modifiers: gdk::ModifierType| {
            let controllers = widget.observe_controllers().snapshot();
            for controller in &controllers {
                if let Some(keys) = controller.downcast_ref::<gtk::EventControllerKey>()
                    && keys.emit_by_name::<bool>("key-pressed", &[&key, &0u32, &modifiers])
                {
                    return;
                }
            }
            for controller in controllers {
                if let Ok(shortcuts) = controller.downcast::<gtk::ShortcutController>() {
                    for shortcut in shortcuts.snapshot() {
                        let shortcut = shortcut.downcast::<gtk::Shortcut>().unwrap();
                        if let Some(trigger) =
                            shortcut.trigger().and_downcast::<gtk::KeyvalTrigger>()
                            && trigger.keyval() == key
                            && trigger.modifiers() == modifiers
                        {
                            assert!(shortcut.action().unwrap().activate(
                                gtk::ShortcutActionFlags::EXCLUSIVE,
                                widget,
                                shortcut.arguments().as_ref(),
                            ));
                            return;
                        }
                    }
                }
            }
            panic!("no binding for {key:?}");
        };
        drain();
        assert_eq!(selection.selected(), 0);
        assert!(RootExt::focus(&window).is_some_and(|focus| focus.is_ancestor(&list)));
        let navigate = |key| {
            press(list.upcast_ref(), key, gdk::ModifierType::empty());
            drain();
        };
        navigate(gdk::Key::Down);
        assert_eq!(selection.selected(), 1);
        navigate(gdk::Key::Up);
        assert_eq!(selection.selected(), 0);
        navigate(gdk::Key::Page_Down);
        assert!(selection.selected() > 1);
        navigate(gdk::Key::Page_Up);
        assert_eq!(selection.selected(), 0);
        navigate(gdk::Key::End);
        assert_eq!(selection.selected(), 4999);
        assert!(view.results.scroll.vadjustment().value() > 0.0);
        assert!(
            descendants::<gtk::Label>(list.upcast_ref())
                .iter()
                .any(|label| label.is_mapped() && label.text() == "Item 4999")
        );
        navigate(gdk::Key::Return);
        let menu = descendants::<gtk::MenuButton>(list.upcast_ref())
            .into_iter()
            .find(|button| button.popover().is_some_and(|popover| popover.is_visible()))
            .unwrap();
        menu.popdown();
        drain();
        navigate(gdk::Key::Home);
        assert_eq!(selection.selected(), 0);
        navigate(gdk::Key::space);
        assert!(
            view.results
                .model
                .is_expanded(&snapshot(Filter::default()).items[0])
        );
        navigate(gdk::Key::space);
        assert_eq!(view.results.model.n_items(), 5000);
        navigate(gdk::Key::Right);
        let expanded_count = view.results.model.n_items();
        assert!(expanded_count > 5000);
        navigate(gdk::Key::Right);
        assert_eq!(view.results.model.n_items(), expanded_count);
        navigate(gdk::Key::Left);
        assert_eq!(view.results.model.n_items(), 5000);
        navigate(gdk::Key::Left);
        assert_eq!(view.results.model.n_items(), 5000);
        navigate(gdk::Key::Down);
        navigate(gdk::Key::space);
        navigate(gdk::Key::Right);
        navigate(gdk::Key::Left);
        assert_eq!(view.results.model.n_items(), 5000);
        navigate(gdk::Key::End);
        press(
            view.widget.upcast_ref(),
            gdk::Key::f,
            gdk::ModifierType::CONTROL_MASK,
        );
        assert!(RootExt::focus(&window).is_some_and(|focus| {
            focus.is_ancestor(&descendants::<gtk::Entry>(view.widget.upcast_ref())[0])
        }));
        let search = descendants::<gtk::Entry>(view.widget.upcast_ref()).remove(0);
        press(
            search.upcast_ref(),
            gdk::Key::Down,
            gdk::ModifierType::empty(),
        );
        drain();
        assert_eq!(selection.selected(), 0);
        assert_eq!(view.results.scroll.vadjustment().value(), 0.0);
        assert!(RootExt::focus(&window).is_some_and(|focus| focus.is_ancestor(&list)));
        navigate(gdk::Key::Down);
        assert_eq!(selection.selected(), 1);
        let mut empty = snapshot(Filter::default());
        empty.items.clear();
        view.set_snapshot(empty);
        drain();
        view.filtering.focus_search();
        assert!(!view.results.focus_first());
        assert!(RootExt::focus(&window).is_some_and(|focus| focus.is_ancestor(&search)));
        window.close();
        window.set_child(None::<&gtk::Widget>);
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn expanded_occurrences_use_ellipsis_menus_and_survive_virtualized_scrolling() {
        use crate::model::session::OccurrencesResponse;
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        let view = ListView::new();
        let weak = Rc::downgrade(&view);
        view.connect_load_rows(move |request| {
            weak.upgrade()
                .unwrap()
                .set_rows(ListRowsResponse {
                    revision: request.revision,
                    result: Ok(request
                        .items
                        .into_iter()
                        .map(|id| {
                            let row = row(&id);
                            (id, Some(row))
                        })
                        .collect()),
                })
                .unwrap();
        });
        let weak = Rc::downgrade(&view);
        view.connect_load_occurrences(move |request| {
            let weak = weak.clone();
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(30), move || {
                weak.upgrade()
                    .unwrap()
                    .set_occurrences(OccurrencesResponse {
                        result: Ok(occurrence_page(&request)),
                        request,
                    })
                    .unwrap();
            });
        });
        let commands = Rc::new(RefCell::new(Vec::new()));
        let recorded = commands.clone();
        view.connect_occurrence_action(move |target, action| {
            recorded.borrow_mut().push((target.item.clone(), action));
        });
        view.set_snapshot(snapshot(Filter::default()));
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(800)
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
        let expand = descendants::<gtk::Box>(view.widget.upcast_ref())
            .into_iter()
            .find(|button| button.tooltip_text().as_deref() == Some("Expand occurrences"))
            .unwrap();
        assert_eq!(expand.cursor().unwrap().name().as_deref(), Some("pointer"));
        let click_row = |content: &gtk::Box, x: f64, y: f64| {
            let click = content
                .observe_controllers()
                .snapshot()
                .into_iter()
                .filter_map(|controller| controller.downcast::<gtk::GestureClick>().ok())
                .find(|click| click.button() == gtk::gdk::BUTTON_PRIMARY)
                .unwrap();
            click.emit_by_name::<()>("released", &[&1i32, &x, &y]);
        };
        let arrow = descendants::<gtk::Image>(expand.upcast_ref())
            .into_iter()
            .find(|image| image.has_css_class("ev_list_disclosure"))
            .unwrap();
        assert_eq!(arrow.icon_name().as_deref(), Some("pan-end-symbolic"));
        click_row(&expand, 20.0, 20.0);
        drain();
        assert_eq!(view.results.model.n_items(), 5012);
        assert!(
            !descendants::<gtk::Label>(view.widget.upcast_ref())
                .iter()
                .any(|label| label.text() == "Occurrences")
        );
        let expanded = descendants::<gtk::Box>(view.widget.upcast_ref())
            .into_iter()
            .find(|content| content.tooltip_text().as_deref() == Some("Collapse occurrences"))
            .unwrap();
        let arrow = descendants::<gtk::Image>(expanded.upcast_ref())
            .into_iter()
            .find(|image| image.has_css_class("ev_list_disclosure"))
            .unwrap();
        assert_eq!(arrow.icon_name().as_deref(), Some("pan-down-symbolic"));
        let base_menu = expanded.last_child().unwrap();
        let bounds = base_menu.compute_bounds(&expanded).unwrap();
        click_row(&expanded, bounds.x() as f64 + 2.0, bounds.y() as f64 + 2.0);
        assert_eq!(
            view.results.model.n_items(),
            5012,
            "menu clicks do not toggle expansion"
        );
        let occurrences: Vec<_> = descendants::<gtk::Box>(view.widget.upcast_ref())
            .into_iter()
            .filter(|widget| widget.is_mapped() && widget.has_css_class("ev_list_occurrence"))
            .collect();
        assert!(!occurrences.is_empty());
        let base_marker = descendants::<gtk::Overlay>(expanded.upcast_ref()).remove(0);
        let marker_x = base_marker.compute_bounds(&view.widget).unwrap().x();
        let mut response_classes = std::collections::BTreeSet::new();
        for content in &occurrences {
            let marker = content
                .first_child()
                .unwrap()
                .downcast::<gtk::Overlay>()
                .unwrap();
            assert_eq!(
                marker.compute_bounds(&view.widget).unwrap().x(),
                marker_x,
                "occurrence circles align below the base calendar circle"
            );
            assert_eq!(marker.width(), base_marker.width());
            assert!(marker.tooltip_text().unwrap().starts_with("Work: "));
            for inner in descendants::<gtk::Box>(marker.upcast_ref()) {
                for class in [
                    "ev_details_good",
                    "ev_details_warn",
                    "ev_details_critical",
                    "ev_response_pending",
                ] {
                    if inner.has_css_class(class) {
                        response_classes.insert(class);
                    }
                }
            }
            let menus = descendants::<gtk::MenuButton>(content.upcast_ref());
            assert_eq!(
                menus.len(),
                1,
                "each occurrence has exactly one ellipsis menu"
            );
            assert!(menus[0].popover().is_none(), "menus remain lazy");
            assert!(
                descendants::<gtk::Button>(content.upcast_ref())
                    .iter()
                    .all(|button| button.is_ancestor(&menus[0])),
                "there are no individual action buttons"
            );
        }
        assert_eq!(
            response_classes.len(),
            4,
            "each occurrence shows its own response state"
        );
        let menu = descendants::<gtk::MenuButton>(occurrences[0].upcast_ref()).remove(0);
        menu.popup();
        drain();
        assert!(menu.popover().is_some());
        menu.activate_action("item.edit-occurrence", None).unwrap();
        assert_eq!(commands.borrow().len(), 1);
        assert_eq!(commands.borrow()[0].0.calendar, "Work");
        assert_eq!(commands.borrow()[0].0.uid, "item-0");
        assert!(commands.borrow()[0].0.recurrence.is_some());
        assert_eq!(commands.borrow()[0].1, MenuAction::EditOccurrence);
        menu.popdown();
        drain();
        let adjustment = view.results.scroll.vadjustment();
        adjustment.set_value(20.0);
        drain();
        let earlier = descendants::<gtk::Button>(view.widget.upcast_ref())
            .into_iter()
            .find(|button| button.label().as_deref() == Some("Load earlier"))
            .unwrap();
        earlier.grab_focus();
        drain();
        let position = adjustment.value();
        assert!(position > 0.0);
        earlier.emit_clicked();
        assert!(earlier.has_focus(), "the loading control retains focus");
        assert_eq!(earlier.label().as_deref(), Some("Loading occurrences…"));
        earlier.emit_clicked();
        drain();
        assert_eq!(view.results.model.n_items(), 5017);
        assert_eq!(
            adjustment.value(),
            position,
            "loading earlier occurrences must not scroll"
        );
        adjustment.set_value(250.0);
        drain();
        let later = descendants::<gtk::Button>(view.widget.upcast_ref())
            .into_iter()
            .find(|button| button.label().as_deref() == Some("Load later"))
            .unwrap();
        later.grab_focus();
        drain();
        let position = adjustment.value();
        assert!(position > 0.0);
        later.emit_clicked();
        assert!(later.has_focus());
        // Scrolling during the request must remain possible; its response preserves
        // the latest user-selected offset rather than the offset at request time.
        adjustment.set_value(position - 30.0);
        let position = adjustment.value();
        drain();
        assert_eq!(
            adjustment.value(),
            position,
            "loading later occurrences must not scroll"
        );
        assert_eq!(view.results.model.n_items(), 5022);
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
        drain();
        wait_until(|| {
            descendants::<gtk::Label>(view.widget.upcast_ref())
                .iter()
                .any(|label| label.text() == "Item 4999")
        });
        assert!(
            descendants::<gtk::Label>(view.widget.upcast_ref())
                .iter()
                .any(|label| label.text() == "Item 4999")
        );
        assert!(descendants::<FixedRow>(view.widget.upcast_ref()).len() < 512);
        adjustment.set_value(0.0);
        drain();
        assert!(
            !descendants::<gtk::Box>(view.widget.upcast_ref())
                .iter()
                .filter(|widget| widget.is_mapped() && widget.has_css_class("ev_list_occurrence"))
                .collect::<Vec<_>>()
                .is_empty()
        );
        view.set_snapshot(snapshot(Filter::default()));
        drain();
        assert_eq!(view.results.model.n_items(), 5022);
        assert!(
            descendants::<FixedRow>(view.widget.upcast_ref())
                .iter()
                .filter(|row| row.is_mapped())
                .all(|row| row.height() == 45)
        );
        let collapse = descendants::<gtk::Box>(view.widget.upcast_ref())
            .into_iter()
            .find(|button| button.tooltip_text().as_deref() == Some("Collapse occurrences"))
            .unwrap();
        click_row(&collapse, 20.0, 20.0);
        drain();
        assert_eq!(view.results.model.n_items(), 5000);
        window.close();
        window.set_child(None::<&gtk::Widget>);
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn action_refreshes_and_exhausted_pages_preserve_scroll_far_from_the_first_row() {
        use crate::model::session::OccurrencesResponse;
        use std::cell::Cell;
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        gtk::Settings::default()
            .unwrap()
            .set_gtk_enable_animations(true);
        let view = ListView::new();
        let weak = Rc::downgrade(&view);
        view.connect_load_rows(move |request| {
            let weak = weak.clone();
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(80), move || {
                weak.upgrade()
                    .unwrap()
                    .set_rows(ListRowsResponse {
                        revision: request.revision,
                        result: Ok(request
                            .items
                            .into_iter()
                            .map(|id| {
                                let row = row(&id);
                                (id, Some(row))
                            })
                            .collect()),
                    })
                    .unwrap();
            });
        });
        let exhausted = Rc::new(Cell::new(false));
        let end = exhausted.clone();
        let weak = Rc::downgrade(&view);
        view.connect_load_occurrences(move |request| {
            let weak = weak.clone();
            let exhausted = end.get();
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(140), move || {
                let mut page = occurrence_page(&request);
                page.more = !exhausted;
                weak.upgrade()
                    .unwrap()
                    .set_occurrences(OccurrencesResponse {
                        request,
                        result: Ok(page),
                    })
                    .unwrap();
            });
        });
        let reorder = Rc::new(Cell::new(false));
        let changed_order = reorder.clone();
        let weak = Rc::downgrade(&view);
        view.connect_occurrence_action(move |_, _| {
            let weak = weak.clone();
            let reorder = changed_order.get();
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(30), move || {
                let mut refreshed = snapshot(Filter::default());
                if reorder {
                    let modified = refreshed.items.remove(1000);
                    refreshed.items.insert(0, modified);
                }
                weak.upgrade().unwrap().set_snapshot(refreshed);
            });
        });
        view.set_snapshot(snapshot(Filter::default()));
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(650)
            .child(&view.widget)
            .build();
        window.present();
        let drain = || {
            for _ in 0..60 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        drain();
        let adjustment = view.results.scroll.vadjustment();
        adjustment.set_value(45000.0);
        drain();
        let id = ItemId {
            calendar: "Work".into(),
            uid: "item-1000".into(),
            recurrence: None,
        };
        view.results.model.toggle(&id, &chrono_tz::Europe::Berlin);
        drain();
        adjustment.set_value(45100.0);
        drain();
        let occurrence = descendants::<gtk::Box>(view.widget.upcast_ref())
            .into_iter()
            .find(|widget| widget.is_mapped() && widget.has_css_class("ev_list_occurrence"))
            .unwrap();
        let menu = descendants::<gtk::MenuButton>(occurrence.upcast_ref()).remove(0);
        menu.popup();
        drain();
        let popup = menu.popover().unwrap();
        let complete = descendants::<gtk::Button>(popup.upcast_ref())
            .into_iter()
            .find(|button| button.action_name().as_deref() == Some("item.complete"))
            .unwrap();
        complete.grab_focus();
        drain();
        let position = adjustment.value();
        complete.emit_clicked();
        drain();
        assert_eq!(
            adjustment.value(),
            position,
            "action refreshes must retain the scroll offset"
        );
        for label in ["Load earlier", "Load later"] {
            // Bring the respective control into view without placing it on the first page.
            adjustment.set_value(if label == "Load earlier" {
                45000.0
            } else {
                45300.0
            });
            drain();
            let button = descendants::<gtk::Button>(view.widget.upcast_ref())
                .into_iter()
                .find(|button| button.label().as_deref() == Some(label))
                .unwrap();
            button.grab_focus();
            drain();
            let position = adjustment.value();
            button.emit_clicked();
            drain();
            assert_eq!(
                adjustment.value(),
                position,
                "loading {label} away from the first page must not scroll"
            );
            exhausted.set(true);
            button.emit_clicked();
            drain();
            assert_eq!(
                adjustment.value(),
                position,
                "exhausting {label} must not scroll"
            );
            exhausted.set(false);
        }
        let occurrence = descendants::<gtk::Box>(view.widget.upcast_ref())
            .into_iter()
            .find(|widget| widget.is_mapped() && widget.has_css_class("ev_list_occurrence"))
            .unwrap();
        let menu = descendants::<gtk::MenuButton>(occurrence.upcast_ref()).remove(0);
        menu.popup();
        drain();
        let complete = descendants::<gtk::Button>(menu.popover().unwrap().upcast_ref())
            .into_iter()
            .find(|button| button.action_name().as_deref() == Some("item.complete"))
            .unwrap();
        complete.grab_focus();
        drain();
        let position = adjustment.value();
        reorder.set(true);
        complete.emit_clicked();
        drain();
        assert_eq!(
            adjustment.value(),
            position,
            "moving a modified series to the top must not scroll"
        );
        window.close();
        window.set_child(None::<&gtk::Widget>);
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn filters_and_virtualized_scrolling_load_only_live_rows_at_fixed_height() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        let weak_model = {
            let view = ListView::new();
            let actions = Rc::new(RefCell::new(Vec::new()));
            let recorded = actions.clone();
            view.connect_action(move |row, action| {
                recorded
                    .borrow_mut()
                    .push((row.calendar.clone(), row.uid.clone(), action));
            });
            let changes = Rc::new(RefCell::new(Vec::new()));
            let weak = Rc::downgrade(&view);
            let recorded = changes.clone();
            view.connect_changed(move || {
                let view = weak.upgrade().unwrap();
                recorded.borrow_mut().push(view.filter());
                view.set_snapshot(snapshot(view.filter()));
            });
            let loads = Rc::new(RefCell::new(Vec::new()));
            let loaded = loads.clone();
            let weak = Rc::downgrade(&view);
            view.connect_load_rows(move |request| {
                let view = weak.upgrade().unwrap();
                loaded
                    .borrow_mut()
                    .push((request.revision, request.items.len()));
                let result = request
                    .items
                    .into_iter()
                    .map(|item| {
                        let row = row(&item);
                        (item, Some(row))
                    })
                    .collect();
                view.set_rows(ListRowsResponse {
                    revision: request.revision,
                    result: Ok(result),
                })
                .unwrap();
            });
            view.set_snapshot(snapshot(Filter::default()));
            let window = gtk::Window::builder()
                .default_width(1000)
                .default_height(500)
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
            assert_eq!(view.results.model.n_items(), 5000);
            let count: usize = loads.borrow().iter().map(|(_, count)| count).sum();
            let page_capacity = (view.results.scroll.vadjustment().page_size()
                / f64::from(super::row::ROW_HEIGHT))
            .ceil() as usize
                + 1;
            assert!(
                (1..=page_capacity).contains(&count),
                "initial loading is limited to the viewport, got {count} for {page_capacity} rows"
            );
            let rows = descendants::<FixedRow>(view.widget.upcast_ref());
            assert!(
                rows.len() < 512,
                "GTK only creates a bounded pool of row widgets"
            );
            assert!(
                rows.iter()
                    .filter(|row| row.is_mapped())
                    .all(|row| row.height() == 45)
            );
            let mut action_width = None;
            for row in rows.iter().filter(|row| {
                row.compute_bounds(&view.results.scroll)
                    .is_some_and(|bounds| {
                        bounds.y() + bounds.height() > 0.0
                            && bounds.y() < view.results.scroll.height() as f32
                    })
            }) {
                let actions = row
                    .first_child()
                    .unwrap()
                    .last_child()
                    .unwrap()
                    .downcast::<gtk::MenuButton>()
                    .unwrap();
                assert_eq!(
                    *action_width.get_or_insert(actions.width()),
                    actions.width()
                );
                assert!(actions.height() <= row.height());
                assert!(actions.width() < 50, "only a compact menu button remains");
                let bounds = actions.compute_bounds(row).unwrap();
                assert!(bounds.x() >= 0.0);
                assert!(bounds.x() + bounds.width() <= row.width() as f32);
                assert_eq!(actions.tooltip_text().as_deref(), Some("Actions"));
                assert!(actions.popover().is_none(), "menus are created lazily");
            }
            let menu = rows
                .iter()
                .find(|row| {
                    row.is_mapped()
                        && descendants::<gtk::Label>(row.upcast_ref())
                            .iter()
                            .any(|label| label.text() == "Item 1")
                })
                .unwrap()
                .first_child()
                .unwrap()
                .last_child()
                .unwrap()
                .downcast::<gtk::MenuButton>()
                .unwrap();
            menu.popup();
            drain();
            let popover = menu.popover().unwrap();
            assert!(popover.is_mapped());
            let delete = descendants::<gtk::Label>(popover.upcast_ref())
                .into_iter()
                .find(|label| label.text() == "Delete")
                .unwrap();
            let delete_button = delete.ancestor(gtk::Button::static_type()).unwrap();
            assert!(delete_button.has_css_class("ev_details_critical"));
            for icon in ["trash", "pencil", "tag"] {
                assert!(
                    descendants::<gtk::Image>(popover.upcast_ref())
                        .iter()
                        .any(|image| {
                            image.is_mapped()
                                && image
                                    .gicon()
                                    .and_downcast::<gtk::gio::FileIcon>()
                                    .is_some_and(|file| {
                                        file.file()
                                            .uri()
                                            .ends_with(&format!("/{icon}-symbolic.svg"))
                                    })
                        }),
                    "{icon} is displayed alongside its menu label"
                );
            }
            let postpone = descendants::<gtk::MenuButton>(popover.upcast_ref())
                .into_iter()
                .find(|button| {
                    descendants::<gtk::Label>(button.upcast_ref())
                        .iter()
                        .any(|label| label.text() == "Postpone")
                })
                .unwrap();
            postpone.popup();
            drain();
            let variants = postpone.popover().unwrap();
            assert!(variants.is_mapped());
            let one_day = descendants::<gtk::Button>(variants.upcast_ref())
                .into_iter()
                .find(|button| {
                    descendants::<gtk::Label>(button.upcast_ref())
                        .iter()
                        .any(|label| label.text() == "1day")
                })
                .unwrap();
            assert!(one_day.is_sensitive());
            assert!(
                descendants::<gtk::Image>(one_day.upcast_ref())
                    .iter()
                    .any(|image| image.is_mapped())
            );
            one_day.emit_clicked();
            drain();
            assert_eq!(
                actions.borrow().as_slice(),
                &[(
                    "Work".into(),
                    "item-1".into(),
                    MenuAction::Task(eventix_state::actions::items::TaskAction::Postpone {
                        days: 1
                    }),
                )]
            );
            assert!(!variants.is_visible());
            assert!(!popover.is_visible());
            let search = descendants::<gtk::Entry>(view.widget.upcast_ref()).remove(0);
            assert!(search.allocated_width() <= 200);
            search.set_text("alpha beta OR gamma");
            drain();
            assert!(
                changes.borrow().is_empty(),
                "typing is a draft until applied"
            );
            search.emit_activate();
            assert_eq!(view.filter().keywords, "alpha beta OR gamma");
            let toggles: Vec<_> = descendants::<gtk::ToggleButton>(view.widget.upcast_ref())
                .into_iter()
                .filter(|button| button.has_css_class("ev_list_calendar"))
                .collect();
            for button in &toggles {
                assert_eq!(button.allocated_height(), search.allocated_height());
                assert_eq!(button.allocated_width(), button.allocated_height());
            }
            toggles[0].set_active(false);
            assert_eq!(view.filter().calendars, ["Home"]);
            let count = changes.borrow().len();
            toggles[1].set_active(false);
            assert!(toggles[1].is_active());
            assert_eq!(
                changes.borrow().len(),
                count,
                "the final calendar remains selected"
            );
            let reset = descendants::<gtk::Button>(view.widget.upcast_ref())
                .into_iter()
                .find(|button| button.tooltip_text().as_deref() == Some("Reset"))
                .unwrap();
            reset.emit_clicked();
            drain();
            assert!(search.text().is_empty());
            assert_eq!(view.filter().calendars, ["Work", "Home"]);
            let offscreen = view.results.model.item(5).unwrap().downgrade();
            let old_revision = loads.borrow().last().unwrap().0;
            let adjustment = view.results.scroll.vadjustment();
            let before_jump = loads.borrow().len();
            adjustment.set_value(adjustment.upper() - adjustment.page_size());
            drain();
            wait_until(|| {
                descendants::<gtk::Label>(view.widget.upcast_ref())
                    .iter()
                    .any(|label| label.text() == "Item 4999")
            });
            let labels = descendants::<gtk::Label>(view.widget.upcast_ref());
            assert!(
                labels.iter().any(|label| label.text() == "Item 4999"),
                "the last item is accessible by scrolling"
            );
            assert_eq!(
                loads.borrow().len(),
                before_jump + 1,
                "a scrollbar-sized jump loads the visible page in one request"
            );
            assert!(
                loads.borrow().last().unwrap().1 <= page_capacity,
                "jumping must not load GTK's offscreen cache"
            );
            assert!(
                offscreen.upgrade().is_none(),
                "offscreen presentation data is released"
            );
            let rows = descendants::<FixedRow>(view.widget.upcast_ref());
            assert!(rows.len() < 512);
            assert!(
                rows.iter()
                    .filter(|row| row.is_mapped())
                    .all(|row| row.height() == 45)
            );
            let last = view
                .results
                .model
                .item(4999)
                .unwrap()
                .downcast::<ListItem>()
                .unwrap();
            let position = adjustment.value();
            let retained_contents: Vec<_> = descendants::<FixedRow>(view.widget.upcast_ref())
                .into_iter()
                .filter(|row| row.is_mapped())
                .map(|row| {
                    let content = row.first_child().unwrap();
                    (row, content)
                })
                .collect();
            view.set_snapshot(snapshot(view.filter()));
            drain();
            assert_eq!(
                adjustment.value(),
                position,
                "refresh preserves the scroll position"
            );
            for (row, content) in retained_contents {
                assert_eq!(row.first_child().as_ref(), Some(&content));
            }
            assert_eq!(
                view.results.model.item(4999).unwrap(),
                last.clone().upcast::<gtk::glib::Object>()
            );
            let last_id = ItemId {
                calendar: "Work".into(),
                uid: "item-4999".into(),
                recurrence: None,
            };
            let mut stale = row(&last_id);
            stale.summary = "STALE".into();
            view.set_rows(ListRowsResponse {
                revision: old_revision,
                result: Ok(vec![(last_id, Some(stale))]),
            })
            .unwrap();
            assert_eq!(last.row().as_ref().unwrap().summary, "Item 4999");
            let count = changes.borrow().len();
            window.set_default_size(1000, 800);
            drain();
            assert_eq!(
                changes.borrow().len(),
                count,
                "resize never reloads the list index"
            );
            assert!(
                descendants::<FixedRow>(view.widget.upcast_ref())
                    .iter()
                    .filter(|row| row.is_mapped())
                    .all(|row| row.height() == 45)
            );
            let mut empty = snapshot(view.filter());
            empty.items.clear();
            view.set_snapshot(empty);
            drain();
            assert_eq!(view.results.model.n_items(), 0);
            assert!(
                descendants::<gtk::Label>(view.widget.upcast_ref())
                    .iter()
                    .any(|label| label.is_mapped() && label.text() == "No items found")
            );
            window.close();
            window.set_child(None::<&gtk::Widget>);
            let weak_model = view.results.model.downgrade();
            drop(view);
            drain();
            weak_model
        };
        assert!(
            weak_model.upgrade().is_none(),
            "the list model is released with the view"
        );
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn clearing_filters_loads_visible_rows_without_scrolling() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        let view = ListView::new();
        let weak = Rc::downgrade(&view);
        view.connect_load_rows(move |request| {
            weak.upgrade()
                .unwrap()
                .set_rows(ListRowsResponse {
                    revision: request.revision,
                    result: Ok(request
                        .items
                        .into_iter()
                        .map(|id| {
                            let row = row(&id);
                            (id, Some(row))
                        })
                        .collect()),
                })
                .unwrap();
        });
        view.set_snapshot(snapshot(Filter::default()));
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(500)
            .child(&view.widget)
            .build();
        window.present();
        let drain = || {
            for _ in 0..20 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        drain();
        for count in [1, 0, 3] {
            let mut filtered = snapshot(Filter::default());
            filtered.filter.keywords = "filtered".into();
            filtered.items = filtered.items.into_iter().skip(100).take(count).collect();
            view.results.reset_scroll();
            view.set_snapshot(filtered);
            drain();
            view.results.reset_scroll();
            view.set_snapshot(snapshot(Filter::default()));
            drain();
            view.set_snapshot(snapshot(Filter::default()));
            drain();
            let adjustment = view.results.scroll.vadjustment();
            assert_eq!(adjustment.value(), 0.0);
            let end = (adjustment.page_size() / f64::from(super::row::ROW_HEIGHT)).ceil() as u32;
            for index in 0..end {
                let item = view
                    .results
                    .model
                    .item(index)
                    .unwrap()
                    .downcast::<ListItem>()
                    .unwrap();
                assert!(
                    item.row().is_some(),
                    "visible row {index} is empty after clearing {count} matches"
                );
            }
        }
        window.close();
        window.set_child(None::<&gtk::Widget>);
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn viewport_loading_merges_pages_skips_cached_rows_and_deduplicates_requests() {
        use super::model::ListModel;

        gtk::init().unwrap();
        let model: ListModel = gtk::glib::Object::new();
        let ids = snapshot(Filter::default()).items;
        model.set_items(ids.clone());
        let retained: Vec<_> = (0..200)
            .map(|index| model.item(index).unwrap().downcast::<ListItem>().unwrap())
            .collect();
        let requests = Rc::new(RefCell::new(Vec::new()));
        let recorded = requests.clone();
        model.connect_load_rows(move |request| recorded.borrow_mut().push(request));
        model.set_viewport(10, 30, false);
        wait_until(|| requests.borrow().len() == 1);
        assert_eq!(requests.borrow()[0].items, ids[10..30]);

        model.set_viewport(11, 31, false);
        wait_until(|| requests.borrow().len() == 2);
        assert_eq!(
            requests.borrow()[1].items,
            ids[30..31],
            "overlapping requests must not duplicate in-flight rows"
        );

        model.set_viewport(100, 120, false);
        while gtk::glib::MainContext::default().iteration(false) {}
        assert_eq!(
            requests.borrow().len(),
            3,
            "page jumps load on the next idle turn"
        );
        assert_eq!(requests.borrow()[2].items, ids[100..120]);
        for range in [10..30, 100..120] {
            model
                .set_rows(ListRowsResponse {
                    revision: 1,
                    result: Ok(ids[range]
                        .iter()
                        .map(|id| (id.clone(), Some(row(id))))
                        .collect()),
                })
                .unwrap();
        }
        assert!(retained[100..120].iter().all(ListItem::loaded));
        assert!(
            retained[..100].iter().all(|item| !item.loaded()),
            "late offscreen results must not update cached widgets"
        );

        model.set_viewport(101, 121, false);
        wait_until(|| requests.borrow().len() == 4);
        assert_eq!(
            requests.borrow()[3].items,
            ids[120..121],
            "already loaded rows must not be requested again"
        );
        model.set_items(ids.clone());
        while gtk::glib::MainContext::default().iteration(false) {}
        assert_eq!(requests.borrow().len(), 5);
        assert_eq!(requests.borrow()[4].revision, 2);
        assert_eq!(
            requests.borrow()[4].items,
            ids[101..121],
            "refresh must reload just the visible page in one request"
        );
        model.toggle(&ids[102], &chrono_tz::Europe::Berlin);
        model
            .set_rows(ListRowsResponse {
                revision: 2,
                result: Ok(ids[101..121]
                    .iter()
                    .map(|id| (id.clone(), Some(row(id))))
                    .collect()),
            })
            .unwrap();
        model.toggle(&ids[102], &chrono_tz::Europe::Berlin);
        while gtk::glib::MainContext::default().iteration(false) {}
        assert_eq!(requests.borrow().len(), 6);
        assert_eq!(
            requests.borrow()[5].items,
            ids[119..121],
            "collapsing a series must load newly exposed cached rows without scrolling"
        );
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn viewport_loading_progresses_during_continuous_small_scroll_movements() {
        use std::{cell::Cell, time::Duration};

        use super::model::ListModel;

        gtk::init().unwrap();
        let model: ListModel = gtk::glib::Object::new();
        model.set_items(snapshot(Filter::default()).items);
        let _retained: Vec<_> = (0..300).map(|index| model.item(index).unwrap()).collect();
        let requests = Rc::new(RefCell::new(Vec::new()));
        let recorded = requests.clone();
        model.connect_load_rows(move |request| recorded.borrow_mut().push(request));
        model.set_viewport(0, 20, false);
        wait_until(|| requests.borrow().len() == 1);

        let movements = Rc::new(Cell::new(0));
        let moved = movements.clone();
        let scrolling = model.clone();
        let source = gtk::glib::timeout_add_local(Duration::from_millis(5), move || {
            let first = moved.get() + 1;
            moved.set(first);
            // A partially visible row must not be mistaken for a viewport resize.
            scrolling.set_viewport(first, first + 20 + first % 2, false);
            gtk::glib::ControlFlow::Continue
        });
        wait_until(|| requests.borrow().len() > 1);
        source.remove();
        assert!(
            (2..100).contains(&movements.get()),
            "small movements should coalesce without starving continuous scrolling"
        );
        let requests = requests.borrow();
        let identities: std::collections::BTreeSet<_> = requests
            .iter()
            .flat_map(|request| request.items.iter().map(|id| (&id.calendar, &id.uid)))
            .collect();
        assert_eq!(
            identities.len(),
            requests.iter().map(|request| request.items.len()).sum(),
            "continuous scrolling must not duplicate in-flight identities"
        );
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn viewport_results_present_the_page_together_and_reject_superseded_work() {
        use super::model::ListModel;

        gtk::init().unwrap();
        let model: ListModel = gtk::glib::Object::new();
        let ids: Vec<_> = snapshot(Filter::default())
            .items
            .into_iter()
            .take(64)
            .collect();
        model.set_items(ids.clone());
        let mut items: Vec<_> = (0..64)
            .map(|index| model.item(index).unwrap().downcast::<ListItem>().unwrap())
            .collect();
        model.set_viewport(0, 20, false);
        let notifications = Rc::new(RefCell::new(Vec::new()));
        for (index, item) in items.iter().enumerate() {
            let recorded = notifications.clone();
            item.connect_notify_local(Some("loaded"), move |_, _| {
                recorded.borrow_mut().push(index);
            });
        }
        let expired = items.remove(0).downgrade();
        assert!(expired.upgrade().is_none());
        let replacement = model.item(0).unwrap().downcast::<ListItem>().unwrap();
        model
            .set_rows(ListRowsResponse {
                revision: 1,
                result: Ok(ids.iter().map(|id| (id.clone(), Some(row(id)))).collect()),
            })
            .unwrap();
        assert_eq!(
            *notifications.borrow(),
            (1..20).collect::<Vec<_>>(),
            "the visible page is presented together without delayed batches"
        );
        assert!(
            replacement.loaded(),
            "responses can populate a recreated visible object"
        );
        assert!(items[19..].iter().all(|item| !item.loaded()));

        let old_summary = items[0].row().as_ref().unwrap().summary.clone();
        model.set_items(ids.clone());
        assert_eq!(
            items[0].row().as_ref().unwrap().summary,
            old_summary,
            "refresh retains existing content"
        );
        model
            .set_rows(ListRowsResponse {
                revision: 1,
                result: Ok(vec![(ids[1].clone(), None)]),
            })
            .unwrap();
        model
            .set_rows(ListRowsResponse {
                revision: 2,
                result: Ok(vec![(ids[2].clone(), None)]),
            })
            .unwrap();
        assert!(items[1].row().is_none());
        assert_eq!(
            items[0].row().as_ref().unwrap().summary,
            old_summary,
            "changing the list rejects late responses"
        );
        let weak = model.downgrade();
        let refreshed_ids = ids.clone();
        let handler = items[2].connect_notify_local(Some("loaded"), move |_, _| {
            weak.upgrade().unwrap().set_items(refreshed_ids.clone());
        });
        let mut obsolete = row(&ids[4]);
        obsolete.summary = "OBSOLETE".into();
        model
            .set_rows(ListRowsResponse {
                revision: 2,
                result: Ok(vec![
                    (ids[3].clone(), Some(row(&ids[3]))),
                    (ids[4].clone(), Some(obsolete)),
                ]),
            })
            .unwrap();
        items[2].disconnect(handler);
        assert_ne!(
            items[3].row().as_ref().unwrap().summary,
            "OBSOLETE",
            "a synchronous list replacement must stop the old response halfway through"
        );
        assert!(
            model
                .set_rows(ListRowsResponse {
                    revision: 3,
                    result: Err("failed load".into()),
                })
                .is_err()
        );
        let weak = model.downgrade();
        drop(model);
        assert!(weak.upgrade().is_none(), "timers must not retain the model");
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn recycled_content_resets_presentation_and_dispatches_only_current_item_actions() {
        use std::sync::Arc;

        use eventix_view::details::Response;

        use super::{content::Content, item::Entry, model::ListModel};

        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let model: ListModel = gtk::glib::Object::new();
        let content = Content::new(&model);
        let actions = Rc::new(crate::view::action_menu::Actions::default());
        let occurrence_actions = Rc::new(crate::view::action_menu::Actions::default());
        let commands = Rc::new(RefCell::new(Vec::new()));
        let recorded = commands.clone();
        actions.connect(move |row: &Row, action| {
            recorded.borrow_mut().push((row.uid.clone(), action));
        });
        let occurrences = Rc::new(RefCell::new(Vec::new()));
        let recorded = occurrences.clone();
        occurrence_actions.connect(
            move |target: &crate::view::action_menu::OccurrenceTarget, action| {
                recorded.borrow_mut().push((target.item.clone(), action));
            },
        );
        let locale = eventix_locale::default();
        let present = |entry, row| {
            let item: ListItem = gtk::glib::Object::new();
            item.set_entry(entry);
            item.set_row(row);
            content.update(&item, &*locale, &actions, &occurrence_actions);
        };
        let id = ItemId {
            calendar: "Work".into(),
            uid: "item-0".into(),
            recurrence: None,
        };
        let mut task = row(&id);
        task.completed = true;
        task.response = Some(eventix_ical::objects::CalPartStat::Declined);
        present(Entry::Base(id.clone()), Some(task));
        let root = content.widget.first_child().unwrap();
        let widgets = descendants::<gtk::Widget>(&root);
        let summary = descendants::<gtk::Label>(&root)
            .into_iter()
            .find(|label| label.text() == "Item 0")
            .unwrap();
        let marker = descendants::<gtk::Overlay>(&root).remove(0);
        let menu = descendants::<gtk::MenuButton>(&root).remove(0);
        assert!(summary.has_css_class("ev_list_completed"));
        assert_eq!(root.tooltip_text().as_deref(), Some("Expand occurrences"));
        let controllers = root.observe_controllers().n_items();
        let click = root
            .observe_controllers()
            .snapshot()
            .into_iter()
            .filter_map(|controller| controller.downcast::<gtk::GestureClick>().ok())
            .find(|click| click.button() == gtk::gdk::BUTTON_PRIMARY)
            .unwrap();
        content.clear();
        assert!(menu.activate_action("item.edit", None).is_err());
        click.emit_by_name::<()>("released", &[&1i32, &0.0f64, &0.0f64]);
        assert!(
            !model.is_expanded(&id),
            "unbound expansion handlers do nothing"
        );

        let event_id = ItemId {
            calendar: "Home".into(),
            uid: "item-1".into(),
            recurrence: None,
        };
        let mut event = row(&event_id);
        event.task = false;
        event.alarm = false;
        event.calendar_name = "Home".into();
        event.background = "#00aa00".into();
        event.date_range.clear();
        event.recurrence.clear();
        event.actions = vec![Action::CopyUid];
        for _ in 0..3 {
            present(Entry::Base(event_id.clone()), Some(event.clone()));
        }
        assert_eq!(
            descendants::<gtk::Widget>(&root),
            widgets,
            "base rows reuse their widget trees"
        );
        assert_eq!(content.widget.first_child().as_ref(), Some(&root));
        assert_eq!(summary.text(), "Item 1");
        assert!(!summary.has_css_class("ev_list_completed"));
        assert_eq!(marker.tooltip_text().as_deref(), Some("Home"));
        assert!(!root.is_focusable());
        assert!(root.tooltip_text().is_none());
        assert!(root.cursor().is_none());
        assert_eq!(root.observe_controllers().n_items(), controllers);
        assert!(menu.activate_action("item.edit", None).is_err());
        menu.activate_action("item.copy-uid", None).unwrap();
        assert_eq!(
            commands.borrow().as_slice(),
            &[("item-1".into(), MenuAction::CopyUid)]
        );
        click.emit_by_name::<()>("released", &[&1i32, &0.0f64, &0.0f64]);
        assert!(!model.is_expanded(&id));
        assert!(!model.is_expanded(&event_id));

        let request = crate::model::session::OccurrencesRequest {
            revision: 0,
            token: 0,
            item: id.clone(),
            cursor: eventix_view::list::occurrences::Cursor {
                start: chrono::Utc::now().with_timezone(locale.timezone()),
                recurrence: String::new(),
            },
            direction: eventix_view::list::occurrences::Direction::From,
            count: 1,
        };
        let mut occurrence = occurrence_page(&request).rows.remove(0);
        Arc::make_mut(&mut occurrence).response = Some(Response::Accepted);
        Arc::make_mut(&mut occurrence).excluded = true;
        let occurrence_id = occurrence.item.clone();
        present(Entry::Occurrence(occurrence), None);
        assert_eq!(content.widget.first_child().as_ref(), Some(&root));
        assert_eq!(root.first_child().as_ref(), Some(marker.upcast_ref()));
        assert!(root.has_css_class("ev_list_occurrence"));
        assert_eq!(root.margin_start(), 32);
        assert!(summary.has_css_class("ev_list_completed"));
        assert_eq!(summary.tooltip_text().as_deref(), Some("Excluded"));
        assert_eq!(descendants::<gtk::MenuButton>(&root)[0], menu);
        menu.activate_action("item.edit-occurrence", None).unwrap();
        assert_eq!(
            occurrences.borrow().as_slice(),
            &[(occurrence_id, MenuAction::EditOccurrence)]
        );
        assert!(
            commands.borrow().len() == 1,
            "occurrence actions do not dispatch base targets"
        );

        present(Entry::Base(event_id.clone()), Some(event.clone()));
        assert_eq!(descendants::<gtk::Widget>(&root), widgets);
        assert!(!root.has_css_class("ev_list_occurrence"));
        assert_eq!(root.margin_start(), 0);
        assert!(!summary.has_css_class("ev_list_completed"));
        assert_eq!(summary.tooltip_text().as_deref(), Some("Item 1"));
        assert!(menu.activate_action("item.edit-occurrence", None).is_err());
        assert!(menu.popover().is_none(), "recycled menus remain lazy");

        let requests = Rc::new(RefCell::new(Vec::new()));
        let recorded = requests.clone();
        model.connect_load_occurrences(move |request| recorded.borrow_mut().push(request));
        model.set_items(vec![id, event_id.clone()]);
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        let german = eventix_locale::new_with_timezone(
            &xdg,
            eventix_locale::LocaleType::German,
            chrono_tz::Europe::Berlin,
        )
        .unwrap();
        event.recurrent = true;
        let item: ListItem = gtk::glib::Object::new();
        item.set_entry(Entry::Base(event_id.clone()));
        item.set_row(Some(event));
        content.update(&item, &*german, &actions, &occurrence_actions);
        assert_eq!(menu.tooltip_text().as_deref(), Some("Aktionen"));
        click.emit_by_name::<()>("released", &[&1i32, &0.0f64, &0.0f64]);
        assert_eq!(requests.borrow().len(), 2);
        for request in requests.borrow().iter() {
            assert_eq!(request.item, event_id);
            assert_eq!(request.cursor.start.timezone(), chrono_tz::Europe::Berlin);
        }
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn recycled_pagination_buttons_use_the_current_series_and_direction() {
        use super::{content::Content, item::Entry, model::ListModel};
        use crate::model::session::OccurrencesResponse;
        use eventix_view::list::occurrences::Direction;

        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let model: ListModel = gtk::glib::Object::new();
        let requests = Rc::new(RefCell::new(Vec::new()));
        let recorded = requests.clone();
        model.connect_load_occurrences(move |request| recorded.borrow_mut().push(request));
        let a = ItemId {
            calendar: "Work".into(),
            uid: "item-0".into(),
            recurrence: None,
        };
        let b = ItemId {
            calendar: "Home".into(),
            uid: "item-2".into(),
            recurrence: None,
        };
        model.set_items(vec![a.clone(), b.clone()]);
        for id in [&a, &b] {
            model.toggle(id, &chrono_tz::Europe::Berlin);
        }
        for request in requests.take() {
            model
                .set_occurrences(OccurrencesResponse {
                    result: Ok(occurrence_page(&request)),
                    request,
                })
                .unwrap();
        }
        let content = Content::new(&model);
        let locale = eventix_locale::default();
        let actions = Rc::new(crate::view::action_menu::Actions::default());
        let occurrence_actions = Rc::new(crate::view::action_menu::Actions::default());
        let present = |id: &ItemId, direction, loading| {
            let item: ListItem = gtk::glib::Object::new();
            item.set_entry(Entry::More(id.clone(), direction, loading));
            content.update(&item, &*locale, &actions, &occurrence_actions);
        };
        present(&a, Direction::Earlier, false);
        let button = content
            .widget
            .first_child()
            .and_downcast::<gtk::Button>()
            .unwrap();
        content.clear();
        button.emit_clicked();
        assert!(requests.borrow().is_empty());
        present(&b, Direction::Later, false);
        assert_eq!(
            content.widget.first_child().as_ref(),
            Some(button.upcast_ref())
        );
        button.emit_clicked();
        let request = requests.take().pop().unwrap();
        assert_eq!(request.item, b);
        assert_eq!(request.direction, Direction::Later);
        present(&b, Direction::Later, true);
        assert_eq!(button.label().as_deref(), Some("Loading occurrences…"));
        button.emit_clicked();
        assert!(
            requests.borrow().is_empty(),
            "loading does not request duplicate pages"
        );
        present(&a, Direction::Earlier, false);
        button.emit_clicked();
        let request = requests.take().pop().unwrap();
        assert_eq!(request.item, a);
        assert_eq!(request.direction, Direction::Earlier);
    }
}
