// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    sync::Arc,
};

use eventix_state::actions::items::ItemId;
use gtk::{glib, prelude::*, subclass::prelude::*};

use crate::model::list::Row;
use crate::model::list_occurrences::{Direction, Row as OccurrenceRow};

#[derive(Clone, Debug)]
pub(super) enum Entry {
    Base(ItemId),
    More(ItemId, Direction, bool),
    Occurrence(Arc<OccurrenceRow>),
    Empty(ItemId),
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Base(a), Self::Base(b)) | (Self::Empty(a), Self::Empty(b)) => a == b,
            // Loading changes must retain the focused control's model identity.
            (Self::More(a, ad, _), Self::More(b, bd, _)) => a == b && ad == bd,
            (Self::Occurrence(a), Self::Occurrence(b)) => a.item == b.item,
            _ => false,
        }
    }
}

mod imp {
    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::ListItem)]
    pub struct ListItem {
        pub row: RefCell<Option<Row>>,
        pub(super) entry: RefCell<Option<Entry>>,
        #[property(get)]
        pub loaded: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ListItem {
        const NAME: &'static str = "EventixListItem";
        type Type = super::ListItem;
    }

    #[glib::derived_properties]
    impl ObjectImpl for ListItem {}
}

glib::wrapper! {
    pub struct ListItem(ObjectSubclass<imp::ListItem>);
}

impl ListItem {
    pub(super) fn entry(&self) -> std::cell::Ref<'_, Option<Entry>> {
        self.imp().entry.borrow()
    }

    pub(super) fn set_entry(&self, entry: Entry) {
        self.imp().entry.replace(Some(entry));
    }

    pub fn row(&self) -> std::cell::Ref<'_, Option<Row>> {
        self.imp().row.borrow()
    }

    pub fn set_row(&self, row: Option<Row>) {
        self.imp().row.replace(row);
        self.imp().loaded.set(true);
        self.notify("loaded");
    }
}
