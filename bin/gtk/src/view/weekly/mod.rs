// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod card;
pub mod timeline;
#[allow(clippy::module_inception)] // The weekly view's main widget shares the view's name.
pub mod weekly;
