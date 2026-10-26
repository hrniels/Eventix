// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{glib, prelude::*};

/// Preserves the pixel offset while GTK lays out inserted or removed list rows.
pub(super) struct ScrollPosition {
    adjustment: gtk::Adjustment,
    list: glib::WeakRef<gtk::ListView>,
    pending: Cell<Option<f64>>,
}

impl ScrollPosition {
    pub fn reset(&self) {
        self.pending.set(None);
        self.adjustment.set_value(0.0);
    }

    pub fn new(scroll: &gtk::ScrolledWindow, list: &gtk::ListView) -> Rc<Self> {
        let position = Rc::new(Self {
            adjustment: scroll.vadjustment(),
            list: list.downgrade(),
            pending: Cell::new(None),
        });
        let weak = Rc::downgrade(&position);
        position
            .adjustment
            .connect_value_changed(move |adjustment| {
                if let Some(position) = weak.upgrade()
                    && let Some(value) = position.pending.get()
                    && adjustment.value() != value
                {
                    adjustment.set_value(value);
                }
            });
        let weak = Rc::downgrade(&position);
        list.connect_unmap(move |_| {
            if let Some(position) = weak.upgrade() {
                position.pending.set(None);
            }
        });
        position
    }

    pub fn hold(self: &Rc<Self>) {
        if self.pending.get().is_some() {
            return;
        }
        let Some(clock) = self.list.upgrade().and_then(|list| list.frame_clock()) else {
            return;
        };
        self.pending.set(Some(self.adjustment.value()));

        // GtkListView can adjust its focused-row anchor during allocation, after the
        // model notification has returned. Hold the offset until that frame is painted.
        let weak = Rc::downgrade(self);
        let handler = Rc::new(RefCell::new(None));
        let id = handler.clone();
        *handler.borrow_mut() = Some(clock.connect_after_paint(move |clock| {
            if let Some(position) = weak.upgrade() {
                position.pending.set(None);
            }
            if let Some(id) = id.take() {
                clock.disconnect(id);
            }
        }));
        clock.request_phase(gtk::gdk::FrameClockPhase::AFTER_PAINT);
    }
}
