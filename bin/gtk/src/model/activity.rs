// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Structured session activity, formatted only at the UI boundary.

use chrono::{DateTime, Utc};
use eventix_locale::Locale;
use eventix_state::actions::items::ItemId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Started,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug)]
pub(crate) struct Activity {
    time: DateTime<Utc>,
    request: u64,
    operation: &'static str,
    phase: Phase,
    items: Vec<ItemId>,
    calendar: Option<String>,
    collection: Option<String>,
    error: Option<String>,
}

impl Activity {
    pub fn new(request: u64, operation: &'static str) -> Self {
        Self {
            time: Utc::now(),
            request,
            operation,
            phase: Phase::Started,
            items: Vec::new(),
            calendar: None,
            collection: None,
            error: None,
        }
    }

    pub fn time(&self) -> DateTime<Utc> {
        self.time
    }

    pub fn request(&self) -> u64 {
        self.request
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn items(&self) -> &[ItemId] {
        &self.items
    }

    pub fn set_items(&mut self, items: Vec<ItemId>) {
        self.items = items;
    }

    pub fn add_item(&mut self, item: ItemId) {
        if !self.items().contains(&item) {
            self.items.push(item);
        }
    }

    pub fn set_calendar(&mut self, calendar: Option<String>) {
        self.calendar = calendar;
    }

    pub fn set_collection(&mut self, collection: Option<String>) {
        self.collection = collection;
    }

    pub fn finish(mut self, phase: Phase, error: Option<String>) -> Self {
        self.time = Utc::now();
        self.phase = phase;
        self.error = error;
        self
    }

    pub fn summary(&self, locale: &dyn Locale) -> String {
        if self.phase == Phase::Started {
            let progress = match self.operation {
                "Calendar enabled" => Some("Enabling calendar…"),
                "Calendar disabled" => Some("Disabling calendar…"),
                "Calendar settings updated" => Some("Saving calendar settings…"),
                "Calendar added" => Some("Adding calendar…"),
                "Calendar reloaded" => Some("Reloading calendar…"),
                "Calendar removed" => Some("Removing calendar…"),
                "Remote calendar deleted" => Some("Deleting remote calendar…"),
                _ => None,
            };
            if let Some(progress) = progress {
                return single_line(locale.translate(progress));
            }
        }
        if self.phase == Phase::Completed
            && !matches!(self.operation, "Synchronizing calendars" | "Authenticating")
        {
            return single_line(locale.translate(self.operation));
        }
        let phase = match self.phase {
            Phase::Failed
                if !matches!(self.operation, "Synchronizing calendars" | "Authenticating") =>
            {
                "Failed: {}"
            }
            Phase::Started => "{} started",
            Phase::Completed => "{} completed",
            Phase::Failed => "{} failed",
            Phase::Cancelled => "{} cancelled",
        };
        single_line(
            &formatx::formatx!(locale.translate(phase), locale.translate(self.operation)).unwrap(),
        )
    }

    pub fn text(&self, locale: &dyn Locale, calendar_name: impl Fn(&str) -> String) -> String {
        let mut text = self.summary(locale);
        if let Some(collection) = &self.collection {
            text.push_str(" — ");
            text.push_str(
                &formatx::formatx!(locale.translate("Collection: {}"), collection).unwrap(),
            );
        }
        if let Some(calendar) = &self.calendar {
            text.push_str(&format!(" — {}", calendar_name(calendar)));
        }
        let mut items = Vec::new();
        for item in &self.items {
            if !items.contains(&item) {
                items.push(item);
            }
        }
        if items.len() > 2 {
            let mut calendars = Vec::new();
            for item in &items {
                if self.calendar.as_ref() != Some(&item.calendar)
                    && !calendars.contains(&item.calendar)
                {
                    calendars.push(item.calendar.clone());
                }
            }
            for calendar in calendars {
                text.push_str(&format!(" — {}", calendar_name(&calendar)));
            }
            text.push_str(" — ");
            text.push_str(&formatx::formatx!(locale.translate("{} items"), items.len()).unwrap());
        } else {
            for item in items {
                text.push_str(" — ");
                text.push_str(
                    &formatx::formatx!(
                        locale.translate("Calendar: {}, UID: {}"),
                        calendar_name(&item.calendar),
                        &item.uid
                    )
                    .unwrap(),
                );
                if let Some(recurrence) = &item.recurrence {
                    text.push_str(
                        &formatx::formatx!(locale.translate(", recurrence: {}"), recurrence)
                            .unwrap(),
                    );
                }
            }
        }
        if let Some(error) = &self.error {
            text.push_str(": ");
            text.push_str(locale.translate(error));
        }
        single_line(&text)
    }
}

pub(crate) fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{Activity, Phase};

    #[test]
    fn accessors_preserve_identity_context_and_terminal_outcome() {
        let mut activity = Activity::new(42, "Item updated");
        assert_eq!(activity.request(), 42);
        assert_eq!(activity.operation, "Item updated");
        assert_eq!(activity.phase(), Phase::Started);
        let started = activity.time();
        let first = item("work", "one");
        activity.set_items(vec![first.clone()]);
        activity.add_item(first.clone());
        activity.add_item(item("work", "two"));
        assert_eq!(activity.items(), &[first, item("work", "two")]);

        let locale = eventix_locale::default();
        activity.set_calendar(Some("personal".into()));
        assert!(
            activity
                .text(locale.as_ref(), str::to_owned)
                .contains(" — personal")
        );
        activity.set_calendar(None);
        assert!(
            !activity
                .text(locale.as_ref(), str::to_owned)
                .contains("personal")
        );
        activity.set_collection(Some("Office".into()));
        assert_eq!(activity.collection.as_deref(), Some("Office"));
        activity.set_collection(None);
        assert_eq!(activity.collection, None);

        let activity = activity.finish(Phase::Failed, Some("failure".into()));
        assert!(activity.time() >= started);
        assert_eq!(activity.phase(), Phase::Failed);
        assert_eq!(activity.error.as_deref(), Some("failure"));
    }

    #[test]
    fn successful_mutations_use_outcome_wording_and_long_requests_keep_lifecycle_wording() {
        let locale = eventix_locale::default();
        for (operation, expected) in [
            ("New event created", "New event created"),
            ("New task created", "New task created"),
            ("Item deleted", "Item deleted"),
            ("Item updated", "Item updated"),
            ("Event moved", "Event moved"),
            (
                "Synchronizing calendars",
                "Synchronizing calendars completed",
            ),
            ("Authenticating", "Authenticating completed"),
        ] {
            let activity = Activity::new(1, operation).finish(Phase::Completed, None);
            assert_eq!(activity.summary(locale.as_ref()), expected);
        }
        let activity =
            Activity::new(1, "Item deleted").finish(Phase::Failed, Some("failure".into()));
        assert_eq!(activity.summary(locale.as_ref()), "Failed: Item deleted");
    }

    fn item(calendar: &str, uid: &str) -> eventix_state::actions::items::ItemId {
        eventix_state::actions::items::ItemId {
            calendar: calendar.into(),
            uid: uid.into(),
            recurrence: None,
        }
    }

    #[test]
    fn messages_use_calendar_names_for_items_and_calendar_wide_activities() {
        let locale = eventix_locale::default();
        let locale = locale.as_ref();
        let name = |id: &str| match id {
            "work-id" => "Work".into(),
            "personal-id" => "Personal".into(),
            _ => id.into(),
        };
        let mut activity = Activity::new(1, "Item updated");
        activity.items = vec![
            item("work-id", "original"),
            item("personal-id", "new-series"),
        ];
        let text = activity.text(locale, name);
        assert!(text.contains("Calendar: Work, UID: original"));
        assert!(text.contains("Calendar: Personal, UID: new-series"));
        assert!(!text.contains("work-id"));
        assert!(!text.contains("personal-id"));

        activity.items.clear();
        activity.calendar = Some("work-id".into());
        assert_eq!(activity.text(locale, name), "Item updated started — Work");
        activity.calendar = Some("unknown".into());
        assert_eq!(
            activity.text(locale, name),
            "Item updated started — unknown"
        );
    }

    #[test]
    fn messages_count_more_than_two_items_and_label_collection_scope() {
        let locale = eventix_locale::default();
        let locale = locale.as_ref();
        let mut activity = Activity::new(1, "Synchronizing calendars");
        activity.calendar = Some("Work".into());
        activity.items = vec![
            item("Work", "one"),
            item("Work", "two"),
            item("Personal", "three"),
        ];
        assert_eq!(
            activity.text(locale, str::to_owned),
            "Synchronizing calendars started — Work — Personal — 3 items"
        );
        activity.items.push(activity.items[0].clone());
        assert!(activity.text(locale, str::to_owned).ends_with("3 items"));

        activity.collection = Some("Office".into());
        activity.calendar = None;
        activity.items.clear();
        assert!(
            activity
                .text(locale, str::to_owned)
                .contains("Collection: Office")
        );
    }

    #[test]
    fn failure_preserves_identity_and_flattens_context_and_error() {
        let mut activity = Activity::new(42, "Item updated");
        activity.items.push(eventix_state::actions::items::ItemId {
            calendar: "Work".into(),
            uid: "abc123".into(),
            recurrence: Some("TU2026-10-13T09:00:00".parse().unwrap()),
        });
        let activity = activity.finish(
            Phase::Failed,
            Some("Could not save:\r\n\tconnection refused\n  caused by server".into()),
        );
        assert_eq!(activity.request, 42);
        assert_eq!(activity.phase, Phase::Failed);
        let text = activity.text(eventix_locale::default().as_ref(), str::to_owned);
        assert!(text.starts_with("Failed: Item updated — Calendar: Work, UID: abc123"));
        assert!(text.contains("recurrence:"));
        assert!(text.ends_with("Could not save: connection refused caused by server"));
        assert!(!text.contains(['\n', '\r', '\t']));
    }
}
