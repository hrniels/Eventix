// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_locale::Locale;
use eventix_view::list::page_count;
use gtk::prelude::*;

/// Page navigation independent of calendar date navigation.
pub(super) struct Pagination {
    pub widget: gtk::Box,
    previous: gtk::Button,
    next: gtk::Button,
    label: gtk::Label,
}

impl Pagination {
    pub fn new() -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        widget.set_halign(gtk::Align::Center);
        let previous = gtk::Button::from_icon_name("go-previous-symbolic");
        let next = gtk::Button::from_icon_name("go-next-symbolic");
        let label = gtk::Label::new(None);
        widget.append(&previous);
        widget.append(&label);
        widget.append(&next);
        Self {
            widget,
            previous,
            next,
            label,
        }
    }

    pub fn connect_changed(&self, changed: impl Fn(bool) + 'static) {
        let changed = std::rc::Rc::new(changed);
        let previous = changed.clone();
        self.previous.connect_clicked(move |_| previous(false));
        self.next.connect_clicked(move |_| changed(true));
    }

    pub fn update(&self, page: usize, total: usize, locale: &dyn Locale) {
        let pages = page_count(total);
        self.previous.set_sensitive(page > 1);
        self.next.set_sensitive(page < pages);
        self.previous
            .set_tooltip_text(Some(locale.translate("Previous page")));
        self.next
            .set_tooltip_text(Some(locale.translate("Next page")));
        self.label.set_text(
            &formatx::formatx!(
                locale.translate("Page {} of {} ({} items)"),
                page,
                pages,
                total
            )
            .unwrap(),
        );
    }
}
