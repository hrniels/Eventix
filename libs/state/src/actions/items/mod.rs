// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod move_event;

use eventix_ical::objects::CalDate;

pub use move_event::MoveEvent;

/// Identifies an occurrence independently of its current placement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemId {
    pub calendar: String,
    pub uid: String,
    pub recurrence: Option<CalDate>,
}
