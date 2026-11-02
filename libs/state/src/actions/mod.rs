// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Validated, persisted application operations.

pub mod calendar;
mod create_component;
pub mod items;

pub use create_component::create_component;

use crate::{EventixState, State};

use items::ItemId;

/// A validated asynchronous operation on shared application state.
///
/// Item actions return an [`ItemId`] by default. Calendar management actions return `()`, and
/// synchronization actions return [`crate::SyncResult`].
#[async_trait::async_trait]
pub trait Action<Output = ItemId>: std::fmt::Debug + Send {
    /// Returns the translation key describing the successful outcome for activity messages.
    fn description(&self) -> &'static str;

    /// Returns the identities whose query membership must be checked before execution.
    ///
    /// These identify existing items, including the source of a copy or a split series.
    fn affected_items(&self) -> &[ItemId] {
        &[]
    }

    /// Executes the operation, acquiring and releasing the state lock as needed.
    ///
    /// Callers must release any state lock before awaiting this operation.
    ///
    /// Item creation, copying, and splitting a series return the new item's identity. Callers should
    /// check query membership before execution using [`Self::affected_items`] and afterward using
    /// the returned identity.
    ///
    /// Returns an error if validation or persistence fails.
    async fn execute(self: Box<Self>, state: &EventixState) -> anyhow::Result<Output>;
}

/// An operation that can execute while holding exclusive access to application state.
///
/// Implementors automatically implement [`Action`], which acquires the state lock before executing
/// the operation. Operations that must release and reacquire the lock implement `Action` directly
/// instead.
#[async_trait::async_trait]
pub trait LockedAction<Output = ItemId>: std::fmt::Debug + Send {
    /// Returns the translation key describing the successful outcome for activity messages.
    fn description(&self) -> &'static str;

    /// Returns the existing item identities whose query membership must be checked before execution.
    fn affected_items(&self) -> &[ItemId] {
        &[]
    }

    /// Executes the operation using the caller's exclusive access to state.
    ///
    /// The caller retains exclusive access across awaits. This method must not attempt to acquire
    /// the shared state lock. Returns an error if validation or persistence fails.
    async fn execute_locked(self: Box<Self>, state: &mut State) -> anyhow::Result<Output>;
}

#[async_trait::async_trait]
impl<T, Output> Action<Output> for T
where
    T: LockedAction<Output> + ?Sized,
{
    fn description(&self) -> &'static str {
        LockedAction::description(self)
    }

    fn affected_items(&self) -> &[ItemId] {
        LockedAction::affected_items(self)
    }

    async fn execute(self: Box<Self>, state: &EventixState) -> anyhow::Result<Output> {
        let mut state = state.lock().await;
        self.execute_locked(&mut state).await
    }
}
