// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Validated, persisted application operations.

pub mod items;

use crate::State;

use items::{ChangeOccurrence, CreateItem, DeleteItem, ItemId, MoveEvent, RespondItem, UpdateItem};

/// A validated application operation to execute against the authoritative state.
#[derive(Debug)]
pub enum Command {
    MoveEvent(MoveEvent),
    CreateItem(Box<CreateItem>),
    UpdateItem(Box<UpdateItem>),
    DeleteItem(DeleteItem),
    RespondItem(RespondItem),
    ChangeOccurrence(ChangeOccurrence),
}

impl Command {
    /// Returns the identities whose query membership must be checked before execution.
    pub fn affected_items(&self) -> &[ItemId] {
        match self {
            Self::MoveEvent(command) => std::slice::from_ref(&command.item),
            Self::CreateItem(_) => &[],
            Self::UpdateItem(command) => std::slice::from_ref(&command.item),
            Self::DeleteItem(command) => std::slice::from_ref(&command.item),
            Self::RespondItem(command) => std::slice::from_ref(&command.item),
            Self::ChangeOccurrence(command) => std::slice::from_ref(&command.item),
        }
    }

    /// Persists the operation and returns the identities to invalidate after a successful commit.
    pub fn execute(self, state: &mut State) -> anyhow::Result<Vec<ItemId>> {
        match self {
            Self::MoveEvent(command) => command.execute(state).map(|item| vec![item]),
            Self::CreateItem(command) => command.execute(state).map(|item| vec![item]),
            Self::UpdateItem(command) => command.execute(state).map(|item| vec![item]),
            Self::DeleteItem(command) => command.execute(state).map(|item| vec![item]),
            Self::RespondItem(command) => command.execute(state).map(|item| vec![item]),
            Self::ChangeOccurrence(command) => command.execute(state).map(|item| vec![item]),
        }
    }
}
