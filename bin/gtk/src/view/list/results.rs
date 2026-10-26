// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use eventix_locale::Locale;
use eventix_view::list::{Row, Snapshot};
use gtk::{glib, prelude::*};

use super::{content::Content, item::ListItem, model::ListModel, scroll::ScrollPosition};
use crate::view::action_menu::{Actions, OccurrenceTarget};

type Bindings = RefCell<HashMap<gtk::ListItem, (ListItem, glib::SignalHandlerId)>>;

/// Renders a virtualized list with presentation data loaded on demand.
pub(super) struct Results {
    pub scroll: gtk::ScrolledWindow,
    pub model: ListModel,
    pub widget: gtk::Stack,
    pub actions: Rc<Actions<Row>>,
    pub occurrence_actions: Rc<Actions<OccurrenceTarget>>,
    empty: gtk::Label,
    locale: Rc<RefCell<Arc<dyn Locale + Send + Sync>>>,
    scroll_position: Rc<ScrollPosition>,
}

impl Results {
    pub fn new() -> Self {
        let model: ListModel = glib::Object::new();
        let locale = Rc::new(RefCell::new(eventix_locale::default()));
        let actions = Rc::new(Actions::default());
        let occurrence_actions = Rc::new(Actions::default());
        let builder = gtk::Builder::from_resource(resource_path!("view/list/results.ui"));
        let list: gtk::ListView = builder.object("list").unwrap();
        let scroll: gtk::ScrolledWindow = builder.object("scroll").unwrap();
        let scroll_position = ScrollPosition::new(&scroll, &list);
        let factory = factory(
            model.clone(),
            locale.clone(),
            actions.clone(),
            occurrence_actions.clone(),
            scroll_position.clone(),
        );
        let selection = gtk::NoSelection::new(Some(model.clone()));
        list.set_model(Some(&selection));
        list.set_factory(Some(&factory));
        Self {
            scroll,
            model,
            widget: builder.object("results").unwrap(),
            actions,
            occurrence_actions,
            empty: builder.object("empty").unwrap(),
            locale,
            scroll_position,
        }
    }

    pub fn reset_scroll(&self) {
        self.scroll_position.reset();
    }

    pub fn update(&self, snapshot: Snapshot) {
        self.empty
            .set_text(snapshot.locale.translate("No items found"));
        self.locale.replace(snapshot.locale);
        let empty = snapshot.items.is_empty();
        self.model.set_items(snapshot.items);
        if empty {
            self.widget.set_visible_child(&self.empty);
        } else {
            self.widget.set_visible_child(&self.scroll);
        }
    }
}

fn factory(
    model: ListModel,
    locale: Rc<RefCell<Arc<dyn Locale + Send + Sync>>>,
    actions: Rc<Actions<Row>>,
    occurrence_actions: Rc<Actions<OccurrenceTarget>>,
    scroll_position: Rc<ScrollPosition>,
) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    let bindings = Rc::new(Bindings::default());
    let slots = Rc::new(RefCell::new(HashMap::<gtk::ListItem, Rc<Content>>::new()));
    let weak = Rc::downgrade(&bindings);
    let position = scroll_position.clone();
    model.connect_changing(move |start, removed| {
        position.hold();
        if let Some(bindings) = weak.upgrade() {
            move_focus_before_change(&bindings, start, removed);
        }
    });
    let setup_slots = slots.clone();
    let setup_model = model.clone();
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        item.set_activatable(false);
        item.set_selectable(false);
        let content = Rc::new(Content::new(&setup_model));
        item.set_child(Some(&content.widget));
        setup_slots.borrow_mut().insert(item.clone(), content);
    });
    let bound = bindings.clone();
    let bind_slots = slots.clone();
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let data = item.item().and_downcast::<ListItem>().unwrap();
        let content = bind_slots.borrow()[item].clone();
        content.update(&data, &**locale.borrow(), &actions, &occurrence_actions);
        let weak = Rc::downgrade(&content);
        let locale = locale.clone();
        let actions = actions.clone();
        let occurrence_actions = occurrence_actions.clone();
        let scroll_position = scroll_position.clone();
        let handler = data.connect_notify_local(Some("loaded"), move |data, _| {
            if let Some(content) = weak.upgrade() {
                scroll_position.hold();
                content.update(data, &**locale.borrow(), &actions, &occurrence_actions);
            }
        });
        bound.borrow_mut().insert(item.clone(), (data, handler));
    });
    let unbind_slots = slots.clone();
    factory.connect_unbind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        if let Some((data, handler)) = bindings.borrow_mut().remove(item) {
            data.disconnect(handler);
        }
        let content = unbind_slots.borrow().get(item).cloned();
        if let Some(content) = content {
            content.clear();
        }
    });
    factory.connect_teardown(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        slots.borrow_mut().remove(item);
        item.set_child(None::<&gtk::Widget>);
    });
    factory
}

fn move_focus_before_change(bindings: &Bindings, start: u32, removed: u32) {
    let bindings = bindings.borrow();
    let Some(focus) = bindings
        .keys()
        .find_map(|item| item.child()?.root()?.focus())
    else {
        return;
    };
    let Some(source) = bindings.keys().find(|item| {
        item.child()
            .is_some_and(|row| focus == row || focus.is_ancestor(&row))
    }) else {
        return;
    };
    let end = start + removed;
    if (removed == 0 && source.position() < start)
        || (removed != 0 && !(start..end).contains(&source.position()))
    {
        return;
    }
    // Keep focus on a surviving, stationary row: removing a focused control sends
    // focus to the first item, while inserting before it makes GTK scroll after it.
    let target = bindings
        .keys()
        .filter(|item| {
            if removed == 0 {
                item.position() < start
            } else {
                !(start..end).contains(&item.position())
            }
        })
        .filter_map(|item| {
            item.child()
                .filter(|row| row.is_mapped())
                .map(|row| (item.position(), row))
        })
        .min_by_key(|(position, _)| position.abs_diff(source.position()));
    drop(bindings);
    if let Some((_, row)) = target {
        row.set_focusable(true);
        row.grab_focus();
    } else if let Some(root) = focus.root() {
        root.set_focus(None::<&gtk::Widget>);
    }
}
