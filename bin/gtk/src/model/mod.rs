// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Session-owned state and query subscriptions, independent of GTK objects.

pub(crate) mod activity;
pub(crate) mod alarms;
pub(crate) mod calendars;
pub(crate) mod import;
pub(crate) mod management;
mod push;
pub(crate) mod query;
pub(crate) mod session;
