// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Validated, persisted application operations.

mod create_component;
pub mod items;

pub use create_component::create_component;

use crate::State;

use items::ItemId;

/// A validated operation that persists a change to an item.
pub trait Action: std::fmt::Debug + Send {
    /// Returns the identities whose query membership must be checked before execution.
    ///
    /// These identify existing items, including the source of a copy or a split series.
    fn affected_items(&self) -> &[ItemId];

    /// Persists the operation and returns the identity to invalidate after a successful commit.
    ///
    /// Creation, copying, and splitting a series return the new item's identity. Callers should
    /// check query membership before execution using [`Self::affected_items`] and afterward using
    /// the returned identity.
    ///
    /// Returns an error if validation or persistence fails.
    fn execute(self: Box<Self>, state: &mut State) -> anyhow::Result<ItemId>;
}
