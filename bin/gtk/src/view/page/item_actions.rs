// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_ical::objects::CalCompType;
use eventix_state::actions::items::{ChangeOccurrence, ChangeTask, EditScope, ItemId, RespondItem};
use eventix_view::list::Row;
use gtk::{prelude::*, subclass::prelude::*};

use super::CalendarPage;
use crate::view::action_menu::{MenuAction, OccurrenceTarget};

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
            MenuAction::CopyUid
            | MenuAction::EditOccurrence
            | MenuAction::EditFollowing
            | MenuAction::RespondSeries(_)
            | MenuAction::Occurrence(_) => {}
        }
    }

    pub(crate) fn occurrence_menu_action(&self, target: &OccurrenceTarget, action: MenuAction) {
        if action == MenuAction::CopyUid {
            self.clipboard().set_text(&target.item.uid);
            return;
        }
        if !self.can_edit() || self.editor_open() {
            return;
        }
        let mut item = target.item.clone();
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
            MenuAction::RespondSeries(response) => {
                item.recurrence = None;
                if self.execute_command(Box::new(RespondItem { item, response })) {
                    self.dismiss_popovers();
                }
            }
            MenuAction::Occurrence(action) => {
                if self.execute_command(Box::new(ChangeOccurrence { item, action })) {
                    self.dismiss_popovers();
                }
            }
            MenuAction::Delete => {
                item.recurrence = None;
                self.imp()
                    .session
                    .confirm_delete(self, item, target.recurrent, &target.summary);
            }
            MenuAction::Edit | MenuAction::EditOccurrence | MenuAction::EditFollowing => {
                let scope = match action {
                    MenuAction::EditOccurrence => EditScope::Occurrence,
                    MenuAction::EditFollowing => EditScope::Following,
                    _ => EditScope::Series,
                };
                self.dismiss_popovers();
                self.imp().editor.edit_item(self, item, scope);
            }
            MenuAction::Alarms => {
                self.dismiss_popovers();
                self.imp().session.load_reminders(
                    item,
                    if target.task {
                        CalCompType::Todo
                    } else {
                        CalCompType::Event
                    },
                );
            }
            MenuAction::CopyUid => {}
        }
    }
}
