// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::Arc,
};

use eventix_locale::Locale;
use gtk::{glib, prelude::*};

use super::{
    content::Content,
    item::{Entry, ListItem},
    model::ListModel,
    row::ROW_HEIGHT,
    scroll::ScrollPosition,
};
use crate::model::list::{Row, Snapshot};
use crate::view::action_menu::{Actions, OccurrenceTarget};

type Bindings = RefCell<HashMap<gtk::ListItem, (ListItem, glib::SignalHandlerId)>>;
type Slots = Rc<RefCell<HashMap<gtk::ListItem, Rc<Content>>>>;

/// Renders a virtualized list with presentation data loaded on demand.
pub(super) struct Results {
    pub scroll: gtk::ScrolledWindow,
    pub model: ListModel,
    pub widget: gtk::Stack,
    pub actions: Rc<Actions<Row>>,
    pub occurrence_actions: Rc<Actions<OccurrenceTarget>>,
    list: gtk::ListView,
    slots: Slots,
    empty: gtk::Label,
    locale: Rc<RefCell<Arc<dyn Locale + Send + Sync>>>,
    scroll_position: Rc<ScrollPosition>,
    reset_pending: Cell<bool>,
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
        let slots = Slots::default();
        let factory = factory(
            &list,
            &slots,
            model.clone(),
            locale.clone(),
            actions.clone(),
            occurrence_actions.clone(),
            scroll_position.clone(),
        );
        let selection = gtk::SingleSelection::new(Some(model.clone()));
        selection.set_can_unselect(false);
        list.set_model(Some(&selection));
        list.set_factory(Some(&factory));
        list.connect_map(|list| {
            list.grab_focus();
        });
        track_viewport(&model, &scroll);
        Self {
            scroll,
            model,
            widget: builder.object("results").unwrap(),
            actions,
            occurrence_actions,
            list,
            slots,
            empty: builder.object("empty").unwrap(),
            locale,
            scroll_position,
            reset_pending: Cell::new(false),
        }
    }

    pub fn reset_scroll(&self) {
        self.reset_pending.set(true);
        self.scroll_position.reset();
    }

    pub fn focus_first(&self) -> bool {
        if self.model.n_items() == 0 {
            return false;
        }
        if let Some(selection) = self.list.model().and_downcast::<gtk::SingleSelection>() {
            selection.set_selected(0);
        }
        self.scroll_position.reset();
        // A jump from the filter can target a row outside GTK's current widget pool.
        // Wait for that row's allocation before moving keyboard focus to it.
        let slots = Rc::downgrade(&self.slots);
        self.list.add_tick_callback(move |list, _| {
            let Some(slots) = slots.upgrade().filter(|_| list.is_mapped()) else {
                return glib::ControlFlow::Break;
            };
            let row = slots
                .borrow()
                .iter()
                .find(|(item, _)| item.position() == 0)
                .and_then(|(_, content)| content.widget.parent());
            if let Some(row) = row {
                row.grab_focus();
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
        true
    }

    pub fn update(&self, snapshot: Snapshot) {
        self.empty
            .set_text(snapshot.locale.translate("No items found"));
        self.locale.replace(snapshot.locale);
        let empty = snapshot.items.is_empty();
        let reset = self.reset_pending.replace(false);
        if reset {
            // A filter reset starts a new viewport. GTK otherwise follows a
            // surviving item's identity to its new index while our scroll guard
            // retains the old offset, leaving visible rows outside the load range.
            self.list.set_model(None::<&gtk::SelectionModel>);
        }
        self.model.set_items(snapshot.items);
        if reset {
            let selection = gtk::SingleSelection::new(Some(self.model.clone()));
            selection.set_can_unselect(false);
            self.list.set_model(Some(&selection));
        }
        if empty {
            self.widget.set_visible_child(&self.empty);
        } else {
            self.widget.set_visible_child(&self.scroll);
        }
    }
}

fn install_keyboard(list: &gtk::ListView, slots: &Slots) {
    let activated = slots.clone();
    list.connect_activate(move |_, position| {
        let content = activated
            .borrow()
            .iter()
            .find(|(item, _)| item.position() == position)
            .map(|(_, content)| content.clone());
        if let Some(content) = content {
            content.open_menu();
        }
    });
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak_list = list.downgrade();
    let targets = slots.clone();
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        let Some(list) = weak_list.upgrade() else {
            return glib::Propagation::Proceed;
        };
        // Menus retain their own keyboard navigation and activation.
        if !modifiers.is_empty()
            || !matches!(
                key,
                gtk::gdk::Key::space
                    | gtk::gdk::Key::Left
                    | gtk::gdk::Key::Right
                    | gtk::gdk::Key::Return
                    | gtk::gdk::Key::KP_Enter
            )
            || list
                .root()
                .and_then(|root| root.focus())
                .is_some_and(|focus| focus.ancestor(gtk::Popover::static_type()).is_some())
        {
            return glib::Propagation::Proceed;
        }
        let Some(selection) = list.model().and_downcast::<gtk::SingleSelection>() else {
            return glib::Propagation::Proceed;
        };
        let content = targets
            .borrow()
            .iter()
            .find(|(item, _)| item.position() == selection.selected())
            .map(|(_, content)| content.clone());
        if let Some(content) = content {
            match key {
                gtk::gdk::Key::space => content.toggle_occurrences(None),
                gtk::gdk::Key::Left => content.toggle_occurrences(Some(false)),
                gtk::gdk::Key::Right => content.toggle_occurrences(Some(true)),
                _ => content.open_menu(),
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    list.add_controller(keys);
}

fn factory(
    list: &gtk::ListView,
    slots: &Slots,
    model: ListModel,
    locale: Rc<RefCell<Arc<dyn Locale + Send + Sync>>>,
    actions: Rc<Actions<Row>>,
    occurrence_actions: Rc<Actions<OccurrenceTarget>>,
    scroll_position: Rc<ScrollPosition>,
) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    let bindings = Rc::new(Bindings::default());
    install_keyboard(list, slots);
    let weak = Rc::downgrade(&bindings);
    let position = scroll_position.clone();
    model.connect_changing(move |start, removed| {
        position.hold_for_change();
        if let Some(bindings) = weak.upgrade() {
            move_focus_before_change(&bindings, start, removed);
        }
    });
    let setup_slots = slots.clone();
    let setup_model = model.clone();
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        item.set_activatable(true);
        item.set_selectable(true);
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
                if matches!(&*data.entry(), Some(Entry::Base(_))) {
                    scroll_position.hold_for_content();
                } else {
                    scroll_position.hold();
                }
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
    let slots = slots.clone();
    factory.connect_teardown(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        slots.borrow_mut().remove(item);
        item.set_child(None::<&gtk::Widget>);
    });
    factory
}

/// Tracks visible rows independently of GTK's larger pool of bound/cache items.
fn track_viewport(model: &ListModel, scroll: &gtk::ScrolledWindow) {
    let weak = model.downgrade();
    let viewport = scroll.downgrade();
    let page_size = Cell::new(0.0);
    let update = Rc::new(move || {
        let (Some(model), Some(scroll)) = (weak.upgrade(), viewport.upgrade()) else {
            return;
        };
        if !scroll.is_mapped() {
            page_size.set(0.0);
            model.set_viewport(0, 0, false);
            return;
        }
        let adjustment = scroll.vadjustment();
        let height = f64::from(ROW_HEIGHT);
        let first = (adjustment.value() / height).floor() as u32;
        let end = ((adjustment.value() + adjustment.page_size()) / height).ceil() as u32;
        let resized = page_size.replace(adjustment.page_size()) != adjustment.page_size();
        model.set_viewport(first, end, resized);
    });
    let changed = update.clone();
    scroll
        .vadjustment()
        .connect_value_changed(move |_| changed());
    let changed = update.clone();
    scroll.vadjustment().connect_changed(move |_| changed());
    let changed = update.clone();
    scroll.connect_map(move |_| changed());
    scroll.connect_unmap(move |_| update());
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
