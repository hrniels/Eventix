// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod filtering;
mod item;
mod model;
mod results;
mod row;

use std::{cell::RefCell, rc::Rc};

use eventix_view::list::{Filter, Snapshot};
use gtk::prelude::*;

use crate::model::session::{ListRowsRequest, ListRowsResponse};

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
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.add_css_class("ev_list_view");
        let filtering = Filtering::new();
        let results = Results::new();
        filtering.update(&[], &Filter::default(), &*eventix_locale::default());
        widget.append(&filtering.widget);
        widget.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        widget.append(&results.widget);
        Rc::new(Self {
            widget,
            filtering,
            results,
            applied: RefCell::default(),
        })
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
                weak.upgrade()
                    .unwrap()
                    .set_rows(ListRowsResponse {
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
            assert!(
                (1..512).contains(&count),
                "initial row data is bounded, got {count}"
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
            for row in rows.iter().filter(|row| row.is_mapped()) {
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
            adjustment.set_value(adjustment.upper() - adjustment.page_size());
            drain();
            let labels = descendants::<gtk::Label>(view.widget.upcast_ref());
            assert!(
                labels.iter().any(|label| label.text() == "Item 4999"),
                "the last item is accessible by scrolling"
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
            view.set_snapshot(snapshot(view.filter()));
            drain();
            assert_eq!(
                adjustment.value(),
                position,
                "refresh preserves the scroll position"
            );
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
}
