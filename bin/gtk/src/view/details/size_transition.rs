// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::{Cell, OnceCell, RefCell};

use gtk::{glib, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct SizeTransition {
        pub stack: OnceCell<gtk::Stack>,
        pub tick: RefCell<Option<gtk::TickCallbackId>>,
        pub from: Cell<f64>,
        pub progress: Cell<f64>,
        pub running: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SizeTransition {
        const NAME: &'static str = "EventixReminderSizeTransition";
        type Type = super::SizeTransition;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for SizeTransition {
        fn constructed(&self) {
            self.parent_constructed();
            let stack = gtk::Stack::builder()
                .hhomogeneous(false)
                .vhomogeneous(false)
                .build();
            stack.set_parent(&*self.obj());
            self.stack.set(stack).expect("stack is initialized once");
            self.obj().set_overflow(gtk::Overflow::Hidden);
        }

        fn dispose(&self) {
            if let Some(tick) = self.tick.take() {
                tick.remove();
            }
            if let Some(stack) = self.stack.get().filter(|stack| stack.parent().is_some()) {
                stack.unparent();
            }
        }
    }

    impl WidgetImpl for SizeTransition {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let for_size = if orientation == gtk::Orientation::Horizontal {
                -1
            } else {
                for_size
            };
            let size = self.stack.get().unwrap().measure(orientation, for_size);
            if orientation == gtk::Orientation::Vertical && self.running.get() {
                let progress = 1.0 - (1.0 - self.progress.get()).powi(3);
                let height = (self.from.get() + (f64::from(size.1) - self.from.get()) * progress)
                    .round() as i32;
                (height, height, -1, -1)
            } else {
                size
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let stack = self.stack.get().unwrap();
            let minimum = stack.measure(gtk::Orientation::Vertical, width).0;
            // Keep the new controls at their normal size while the container reveals more space.
            stack.allocate(width, height.max(minimum), baseline, None);
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj()
                .snapshot_child(self.stack.get().unwrap(), snapshot);
        }
    }
}

glib::wrapper! {
    pub struct SizeTransition(ObjectSubclass<imp::SizeTransition>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SizeTransition {
    pub(super) fn new() -> Self {
        glib::Object::new()
    }

    pub(super) fn stack(&self) -> &gtk::Stack {
        self.imp().stack.get().unwrap()
    }

    pub(super) fn show(&self, child: &impl IsA<gtk::Widget>) {
        let child = child.as_ref();
        if self.stack().visible_child().as_ref() == Some(child) {
            return;
        }
        let imp = self.imp();
        if let Some(tick) = imp.tick.take() {
            tick.remove();
        }
        imp.from.set(f64::from(self.height()));
        imp.progress.set(0.0);
        imp.running
            .set(self.is_mapped() && self.settings().property::<bool>("gtk-enable-animations"));
        self.stack().set_visible_child(child);
        self.queue_resize();
        if !imp.running.get() {
            return;
        }

        let start = self.frame_clock().unwrap().frame_time();
        let tick = self.add_tick_callback(move |obj, clock| {
            let progress = ((clock.frame_time() - start) as f64 / 200_000.0).clamp(0.0, 1.0);
            obj.imp().progress.set(progress);
            obj.queue_resize();
            if progress == 1.0 {
                obj.imp().running.set(false);
                obj.imp().tick.take();
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
        imp.tick.replace(Some(tick));
    }
}
