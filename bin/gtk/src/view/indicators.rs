// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::Cell;

use eventix_locale::Locale;
use gtk::{glib, prelude::*, subclass::prelude::*};

const ICONS: [(&str, &str); 3] = [
    ("arrow-clockwise", "Recurring event"),
    ("pencil", "Recurrence exception"),
    ("alarm", "Reminder"),
];

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct EventIndicators {
        pub icons: [gtk::Image; 3],
        pub present: Cell<[bool; 3]>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EventIndicators {
        const NAME: &'static str = "EventixEventIndicators";
        type Type = super::EventIndicators;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for EventIndicators {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();
            obj.set_spacing(3);
            for (image, (name, _)) in self.icons.iter().zip(ICONS) {
                image.set_resource(Some(&format!(
                    "/io/github/hrniels/Eventix/gtk/icons/{name}-symbolic.svg"
                )));
                image.set_pixel_size(16);
                image.set_visible(false);
                obj.append(image);
            }

            obj.set_visible(false);
        }
    }

    impl WidgetImpl for EventIndicators {}

    impl BoxImpl for EventIndicators {}
}

glib::wrapper! {
    pub struct EventIndicators(ObjectSubclass<imp::EventIndicators>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl EventIndicators {
    pub fn set_state(&self, recurrent: bool, exception: bool, alarm: bool, locale: &dyn Locale) {
        let present = [recurrent, exception, alarm];
        self.imp().present.set(present);

        for ((image, (_, title)), present) in self.imp().icons.iter().zip(ICONS).zip(present) {
            let title = locale.translate(title);
            image.set_tooltip_text(Some(title));
            image.update_property(&[gtk::accessible::Property::Label(title)]);
            image.set_visible(present);
        }

        self.set_visible(present.into_iter().any(|present| present));
    }

    pub fn set_pixel_size(&self, size: i32) {
        for image in &self.imp().icons {
            image.set_pixel_size(size);
        }
    }

    /// Shows only present indicators that fit within the available content area.
    pub fn fit(&self, mut width: i32, height: i32) {
        let mut count = 0;
        for (image, present) in self.imp().icons.iter().zip(self.imp().present.get()) {
            // Restore visibility before measuring so icons can reappear on resize.
            image.set_visible(present);
            if !present {
                continue;
            }

            let icon_width = image.measure(gtk::Orientation::Horizontal, -1).1;
            let icon_height = image.measure(gtk::Orientation::Vertical, -1).1;
            let needed = icon_width + if count > 0 { self.spacing() } else { 0 };
            let fits = needed <= width && icon_height <= height;
            image.set_visible(fits);
            if fits {
                width -= needed;
                count += 1;
            }
        }

        self.set_visible(count > 0);
    }
}
