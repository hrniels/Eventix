// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Ordered state operations and background execution, independent of GTK objects.

pub(crate) mod alarms;
pub(crate) mod event;
pub(crate) mod import;
pub(crate) mod management;
pub(crate) mod message;
mod push;
mod runtime;
pub(crate) mod session;
pub(crate) mod subscriptions;
