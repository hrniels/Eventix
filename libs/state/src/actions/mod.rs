// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Validated, persisted application operations.

pub mod items;

use crate::State;

use items::{CreateEvent, ItemId, MoveEvent, UpdateEvent};

/// A validated application operation to execute against the authoritative state.
#[derive(Debug)]
pub enum Command {
    MoveEvent(MoveEvent),
    CreateEvent(Box<CreateEvent>),
    UpdateEvent(Box<UpdateEvent>),
}

impl Command {
    /// Returns the identities whose query membership must be checked before execution.
    pub fn affected_items(&self) -> &[ItemId] {
        match self {
            Self::MoveEvent(command) => std::slice::from_ref(&command.item),
            Self::CreateEvent(_) => &[],
            Self::UpdateEvent(command) => std::slice::from_ref(&command.item),
        }
    }

    /// Persists the operation and returns the identities to invalidate after a successful commit.
    pub fn execute(self, state: &mut State) -> anyhow::Result<Vec<ItemId>> {
        match self {
            Self::MoveEvent(command) => command.execute(state).map(|item| vec![item]),
            Self::CreateEvent(command) => command.execute(state).map(|item| vec![item]),
            Self::UpdateEvent(command) => command.execute(state).map(|item| vec![item]),
        }
    }
}
