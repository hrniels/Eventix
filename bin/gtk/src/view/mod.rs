// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod action_menu;
pub(crate) mod auth;
mod calendar_selector;
mod calendars;
pub(crate) mod card;
mod creation_hint;
pub mod details;
pub(crate) mod editor;
mod geometry;
mod indicators;
mod list;
pub(crate) mod monthly;
pub(crate) mod page;
pub(crate) mod popover;
mod response_marker;
mod sidebar;
mod status_bar;
mod surface;
pub mod weekly;

pub(crate) const ITEM_PANEL_WIDTH: i32 = 600;
