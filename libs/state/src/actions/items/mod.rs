// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod create;
mod create_item;
mod move_event;
mod update_item;

use eventix_ical::objects::CalDate;

pub use create::create_component;
pub use create_item::{CreateItem, TaskStatus};
pub use move_event::MoveEvent;
pub use update_item::{EditScope, UpdateItem};

/// Identifies an occurrence independently of its current placement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemId {
    pub calendar: String,
    pub uid: String,
    pub recurrence: Option<CalDate>,
}
