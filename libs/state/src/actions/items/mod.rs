// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod change_occurrence;
mod change_task;
mod create;
mod create_item;
mod delete_item;
mod move_event;
mod respond_item;
mod update_item;

use eventix_ical::objects::CalDate;

pub use change_occurrence::{ChangeOccurrence, OccurrenceAction};
pub use change_task::{ChangeTask, TaskAction};
pub use create::create_component;
pub use create_item::{CreateItem, TaskStatus};
pub use delete_item::DeleteItem;
pub use move_event::{MoveEvent, ResizeEvent};
pub use respond_item::RespondItem;
pub use update_item::{EditScope, UpdateItem};

/// Identifies an occurrence independently of its current placement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemId {
    pub calendar: String,
    pub uid: String,
    pub recurrence: Option<CalDate>,
}
