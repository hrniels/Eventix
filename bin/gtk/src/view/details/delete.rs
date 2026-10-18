// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;

use eventix_locale::Locale;
use formatx::formatx;
use gtk::prelude::*;

/// Prompts before deleting an item and invokes the callback only on explicit confirmation.
// AlertDialog does not expose a message area for adding the delete icon.
#[allow(deprecated)]
pub(crate) fn confirm(
    parent: &gtk::Window,
    locale: &dyn Locale,
    recurrent: bool,
    summary: &str,
    on_confirm: impl FnOnce() + 'static,
) {
    let body = formatx!(
        locale.translate(if recurrent {
            "Deletes the series \"{}\"."
        } else {
            "Deletes the item \"{}\"."
        }),
        summary
    )
    .unwrap();

    let dialog = gtk::MessageDialog::builder()
        .text(locale.translate("Are you sure?"))
        .secondary_text(body)
        .transient_for(parent)
        .modal(true)
        .destroy_with_parent(true)
        .resizable(false)
        .default_width(560)
        .build();

    let icon = gtk::Image::from_resource("/io/github/hrniels/Eventix/gtk/icons/trash-symbolic.svg");
    icon.set_pixel_size(32);
    icon.set_valign(gtk::Align::Center);

    let area = dialog.message_area();
    if let Some(parent) = area.parent().and_downcast::<gtk::Box>() {
        let previous = area.prev_sibling();
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        parent.remove(&area);
        row.append(&icon);
        area.set_hexpand(true);
        row.append(&area);
        // Balance the icon so the text stays centered within the whole dialog.
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let balance = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
        balance.add_widget(&icon);
        balance.add_widget(&spacer);
        row.append(&spacer);
        parent.insert_child_after(&row, previous.as_ref());
    }

    dialog.add_button(locale.translate("Cancel"), gtk::ResponseType::Cancel);
    dialog.add_button(locale.translate("Delete"), gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Cancel);

    let on_confirm = RefCell::new(Some(on_confirm));
    dialog.connect_response(move |dialog, response| {
        dialog.close();
        if response == gtk::ResponseType::Accept
            && let Some(on_confirm) = on_confirm.take()
        {
            on_confirm();
        }
    });

    dialog.present();
}
