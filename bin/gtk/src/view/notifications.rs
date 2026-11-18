// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;

use gtk::{gio, glib, prelude::*};

use crate::model::notifications::Notification;

/// Requests a critical reminder that stays visible until dismissed.
///
/// Desktop notification services may override the expiration request. If direct delivery is
/// unavailable, falls back to GNotification, which also supports sandboxed notification portals.
pub(super) fn send(application: &gtk::Application, alarm: Notification) {
    let Some(connection) = application.dbus_connection() else {
        send_fallback(application, &alarm);
        return;
    };
    // GNotification exposes urgency but not the freedesktop expiration timeout.
    let parameters = parameters(
        application.application_id().as_deref().unwrap_or_default(),
        &alarm,
    );
    let application = application.clone();
    connection.call(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "Notify",
        Some(&parameters),
        None,
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
        move |result| {
            if let Err(error) = result {
                tracing::warn!(%error, "Unable to send persistent reminder; using GNotification");
                send_fallback(&application, &alarm);
            }
        },
    );
}

fn parameters(application_id: &str, alarm: &Notification) -> glib::Variant {
    let application_name = glib::application_name().unwrap_or_else(|| "Eventix".into());
    let body = glib::markup_escape_text(&alarm.body);
    let hints = HashMap::from([
        ("desktop-entry", application_id.to_variant()),
        ("urgency", 2_u8.to_variant()),
    ]);
    (
        application_name.as_str(),
        0_u32,
        "",
        &alarm.summary,
        body.as_str(),
        Vec::<String>::new(),
        hints,
        0_i32, // The freedesktop protocol defines zero as never expiring.
    )
        .to_variant()
}

fn send_fallback(application: &gtk::Application, alarm: &Notification) {
    let notification = gio::Notification::new(&alarm.summary);
    notification.set_body(Some(&alarm.body));
    notification.set_priority(gio::NotificationPriority::Urgent);
    application.send_notification(None, &notification);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminders_request_critical_urgency_and_no_expiration() {
        let alarm = Notification {
            summary: "Meeting".into(),
            body: "Location: <Office> & home".into(),
        };
        let parameters = parameters("io.github.hrniels.Eventix", &alarm);
        assert_eq!(parameters.type_().as_str(), "(susssasa{sv}i)");
        assert_eq!(parameters.child_value(1).get::<u32>(), Some(0));
        assert_eq!(parameters.child_value(3).str(), Some("Meeting"));
        assert_eq!(
            parameters.child_value(4).str(),
            Some("Location: &lt;Office&gt; &amp; home")
        );
        let hints = parameters
            .child_value(6)
            .get::<HashMap<String, glib::Variant>>()
            .unwrap();
        assert_eq!(hints["urgency"].get::<u8>(), Some(2));
        assert_eq!(
            hints["desktop-entry"].str(),
            Some("io.github.hrniels.Eventix")
        );
        assert_eq!(parameters.child_value(7).get::<i32>(), Some(0));
    }
}
