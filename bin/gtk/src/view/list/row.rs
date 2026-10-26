// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::{glib, prelude::*, subclass::prelude::*};

pub(super) const ROW_HEIGHT: i32 = 45;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct FixedRow;

    #[glib::object_subclass]
    impl ObjectSubclass for FixedRow {
        const NAME: &'static str = "EventixListRow";
        type Type = super::FixedRow;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for FixedRow {}

    impl WidgetImpl for FixedRow {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            if orientation == gtk::Orientation::Vertical {
                (ROW_HEIGHT, ROW_HEIGHT, -1, -1)
            } else {
                self.obj()
                    .first_child()
                    .map(|child| child.measure(orientation, for_size))
                    .unwrap_or((0, 0, -1, -1))
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.obj().first_child() {
                child.allocate(width, height, baseline, None);
            }
        }
    }

    impl BoxImpl for FixedRow {}
}

glib::wrapper! {
    pub struct FixedRow(ObjectSubclass<imp::FixedRow>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl FixedRow {
    pub fn new() -> Self {
        let row: Self = glib::Object::builder()
            .property("overflow", gtk::Overflow::Hidden)
            .build();
        // The content must not change the list's uniform row geometry.
        row.set_layout_manager(None::<gtk::LayoutManager>);
        row
    }

    pub fn set_child(&self, child: &impl IsA<gtk::Widget>) {
        if self.first_child().as_ref() == Some(child.as_ref()) {
            return;
        }
        if let Some(old) = self.first_child() {
            // Re-rendering a focused menu/row must not send GTK's list focus back
            // to the first item. The row outlives its presentation widgets.
            if self
                .root()
                .and_then(|root| root.focus())
                .is_some_and(|focus| focus == old || focus.is_ancestor(&old))
            {
                self.set_focusable(true);
                self.grab_focus();
            }
            self.remove(&old);
        }
        self.append(child);
    }
}
