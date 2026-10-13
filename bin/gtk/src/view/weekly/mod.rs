// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The week page coordinates two layouts and one shared details popup.
//!
//! Span construction and collision layout live in eventix-view. The widgets here only render
//! those placements; surface handles card ownership, loader handles I/O, and popover handles GTK
//! popup lifecycle and placement.

pub mod all_day_strip;
pub mod card;
mod geometry;
mod loader;
pub mod page;
pub(crate) mod popover;
mod surface;
pub mod timed_grid;
