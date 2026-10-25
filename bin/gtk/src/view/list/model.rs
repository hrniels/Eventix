// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use eventix_state::actions::items::ItemId;
use gtk::{gio, glib, prelude::*, subclass::prelude::*};

use crate::model::session::{ListRowsRequest, ListRowsResponse};

use super::item::ListItem;

type Key = (String, String);
type LoadRows = Rc<dyn Fn(ListRowsRequest)>;

fn key(item: &ItemId) -> Key {
    (item.calendar.clone(), item.uid.clone())
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct ListModel {
        pub items: RefCell<Vec<ItemId>>,
        pub live: RefCell<BTreeMap<Key, glib::WeakRef<ListItem>>>,
        pub pending: RefCell<BTreeSet<Key>>,
        pub scheduled: Cell<bool>,
        pub revision: Cell<u64>,
        pub(super) load: RefCell<Option<LoadRows>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ListModel {
        const NAME: &'static str = "EventixListModel";
        type Type = super::ListModel;
        type Interfaces = (gio::ListModel,);
    }

    impl ObjectImpl for ListModel {}

    impl ListModelImpl for ListModel {
        fn item_type(&self) -> glib::Type {
            ListItem::static_type()
        }

        fn n_items(&self) -> u32 {
            self.items.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            let key = key(self.items.borrow().get(position as usize)?);
            if let Some(item) = self
                .live
                .borrow()
                .get(&key)
                .and_then(glib::WeakRef::upgrade)
            {
                return Some(item.upcast());
            }
            let item: ListItem = glib::Object::new();
            self.live.borrow_mut().insert(key.clone(), item.downgrade());
            self.obj().queue(key);
            Some(item.upcast())
        }
    }
}

glib::wrapper! {
    pub struct ListModel(ObjectSubclass<imp::ListModel>)
        @implements gio::ListModel;
}

impl ListModel {
    pub(super) fn connect_load_rows(&self, load: impl Fn(ListRowsRequest) + 'static) {
        self.imp().load.replace(Some(Rc::new(load)));
    }

    pub fn set_items(&self, items: Vec<ItemId>) {
        let imp = self.imp();
        imp.revision.set(imp.revision.get() + 1);
        imp.pending.borrow_mut().clear();
        let old = imp.items.replace(items);
        let (prefix, removed, added, keys) = {
            let items = imp.items.borrow();
            let prefix = old
                .iter()
                .zip(items.iter())
                .take_while(|(a, b)| a == b)
                .count();
            let suffix = old[prefix..]
                .iter()
                .rev()
                .zip(items[prefix..].iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            (
                prefix,
                old.len() - prefix - suffix,
                items.len() - prefix - suffix,
                items.iter().map(key).collect::<BTreeSet<_>>(),
            )
        };
        imp.live
            .borrow_mut()
            .retain(|key, item| keys.contains(key) && item.upgrade().is_some());
        if removed != 0 || added != 0 {
            self.items_changed(prefix as u32, removed as u32, added as u32);
        }
        // Refresh only objects retained by GTK's current viewport/cache. Weak
        // references let rows and their presentation data disappear after unbind.
        let live: Vec<_> = imp.live.borrow().keys().cloned().collect();
        for key in live {
            self.queue(key);
        }
    }

    fn queue(&self, key: Key) {
        let imp = self.imp();
        imp.pending.borrow_mut().insert(key);
        if imp.scheduled.replace(true) {
            return;
        }
        let weak = self.downgrade();
        glib::idle_add_local_once(move || {
            if let Some(model) = weak.upgrade() {
                model.load_pending();
            }
        });
    }

    fn load_pending(&self) {
        let imp = self.imp();
        imp.scheduled.set(false);
        imp.live
            .borrow_mut()
            .retain(|_, item| item.upgrade().is_some());
        let items = imp
            .pending
            .take()
            .into_iter()
            .filter(|key| imp.live.borrow().contains_key(key))
            .map(|(calendar, uid)| ItemId {
                calendar,
                uid,
                recurrence: None,
            })
            .collect::<Vec<_>>();
        if !items.is_empty()
            && let Some(load) = imp.load.borrow().clone()
        {
            load(ListRowsRequest {
                revision: imp.revision.get(),
                items,
            });
        }
    }

    pub(super) fn set_rows(&self, response: ListRowsResponse) -> Result<(), String> {
        let imp = self.imp();
        if response.revision != imp.revision.get() {
            return Ok(());
        }
        for (id, row) in response.result? {
            let item = imp
                .live
                .borrow()
                .get(&key(&id))
                .and_then(glib::WeakRef::upgrade);
            if let Some(item) = item {
                item.set_row(row);
            }
        }
        Ok(())
    }
}
