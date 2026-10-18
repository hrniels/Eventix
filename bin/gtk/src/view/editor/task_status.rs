// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_ical::objects::{CalDate, CalDateType, CalTodoStatus};
use eventix_state::actions::items::TaskStatus;
use eventix_view::editor::EditorOptions;
use gtk::prelude::*;

use super::{DateInput, dropdown, layout::row, spin};

pub(super) struct TaskStatusInput {
    pub status: gtk::DropDown,
    percent: gtk::SpinButton,
    pub completed: DateInput,
    pub has_completed: gtk::CheckButton,
}

impl TaskStatusInput {
    pub fn new(form: &gtk::Box, options: &EditorOptions) -> Self {
        let locale = options.locale.as_ref();
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let status = dropdown(
            locale,
            &["NEEDS-ACTION", "COMPLETED", "IN-PROCESS", "CANCELLED"],
        );
        widget.append(&status);
        let progress = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let percent = spin(0.0, 100.0, 0.0);
        progress.append(&percent);
        progress.append(&gtk::Label::new(Some("%")));
        widget.append(&progress);
        progress.set_visible(false);
        let completion = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let has_completed = gtk::CheckButton::with_label(locale.translate("Completed"));
        let completed = DateInput::new(locale, options.start.date());
        completed.widget.set_sensitive(false);
        let field = completed.widget.clone();
        has_completed.connect_toggled(move |button| field.set_sensitive(button.is_active()));
        completion.append(&has_completed);
        completion.append(&completed.widget);
        widget.append(&completion);
        completion.set_visible(false);
        status.connect_selected_notify(move |status| {
            progress.set_visible(status.selected() == 2);
            completion.set_visible(status.selected() == 1);
        });
        row(form, locale, "Status", &widget);
        Self {
            status,
            percent,
            completed,
            has_completed,
        }
    }

    pub fn value(&self, completed: Option<CalDate>) -> TaskStatus {
        TaskStatus {
            status: match self.status.selected() {
                1 => CalTodoStatus::Completed,
                2 => CalTodoStatus::InProcess,
                3 => CalTodoStatus::Cancelled,
                _ => CalTodoStatus::NeedsAction,
            },
            percent: match self.status.selected() {
                1 => Some(100),
                2 => Some(self.percent.value_as_int() as u8),
                _ => None,
            },
            completed,
        }
    }

    pub fn load(&self, status: &TaskStatus) {
        self.status.set_selected(match status.status {
            CalTodoStatus::Completed => 1,
            CalTodoStatus::InProcess => 2,
            CalTodoStatus::Cancelled => 3,
            CalTodoStatus::NeedsAction => 0,
        });
        self.percent
            .set_value(f64::from(status.percent.unwrap_or(0)));
        self.has_completed.set_active(status.completed.is_some());
        if let Some(date) = &status.completed {
            self.completed
                .entry
                .set_text(&date.as_naive_date().to_string());
        }
    }

    pub fn completion_date(&self, date: chrono::NaiveDate) -> CalDate {
        CalDate::new_date(date, CalDateType::Inclusive).to_utc()
    }
}
