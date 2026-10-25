// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::{Cell, RefCell};

use eventix_view::list::Row;
use gtk::{glib, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::ListItem)]
    pub struct ListItem {
        pub row: RefCell<Option<Row>>,
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
    pub fn row(&self) -> std::cell::Ref<'_, Option<Row>> {
        self.imp().row.borrow()
    }

    pub fn set_row(&self, row: Option<Row>) {
        self.imp().row.replace(row);
        self.imp().loaded.set(true);
        self.notify("loaded");
    }
}
