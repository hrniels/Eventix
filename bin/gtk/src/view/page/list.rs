// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_ical::objects::CalCompType;
use eventix_state::actions::items::{ChangeTask, EditScope, ItemId, RespondItem};
use eventix_view::list::Row;
use gtk::{prelude::*, subclass::prelude::*};

use super::CalendarPage;
use crate::view::list::MenuAction;

impl CalendarPage {
    pub(super) fn list_action(&self, row: &Row, action: MenuAction) {
        if action == MenuAction::CopyUid {
            self.clipboard().set_text(&row.uid);
            return;
        }
        if !self.can_edit() || self.editor_open() {
            return;
        }

        let item = ItemId {
            calendar: row.calendar.clone(),
            uid: row.uid.clone(),
            recurrence: None,
        };
        match action {
            MenuAction::Task(action) => {
                if self.execute_command(Box::new(ChangeTask { item, action })) {
                    self.dismiss_popovers();
                }
            }
            MenuAction::Respond(response) => {
                if self.execute_command(Box::new(RespondItem { item, response })) {
                    self.dismiss_popovers();
                }
            }
            MenuAction::Delete => {
                self.imp()
                    .session
                    .confirm_delete(self, item, row.recurrent, &row.summary);
            }
            MenuAction::Edit => {
                self.dismiss_popovers();
                self.imp().editor.edit_item(self, item, EditScope::Series);
            }
            MenuAction::Alarms => {
                self.dismiss_popovers();
                let kind = if row.task {
                    CalCompType::Todo
                } else {
                    CalCompType::Event
                };
                self.imp().session.load_reminders(item, kind);
            }
            MenuAction::CopyUid => {}
        }
    }
}
