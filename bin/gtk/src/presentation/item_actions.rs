// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Available event and task actions shared by item presentations.

use eventix_ical::objects::CalPartStat;

/// An available item action or invitation status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Complete,
    Postpone,
    Delete,
    Edit,
    Respond {
        status: CalPartStat,
        read_only: bool,
    },
    Alarms,
    CopyUid,
}
