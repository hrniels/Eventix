// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

// Public re-exports of internal modules used by the integration tests in tests/.
// The binary target (main.rs) uses private `mod` declarations for the same modules;
// this library target makes a subset of them accessible to test binaries.

pub mod api;
pub mod comps;
pub mod extract;
pub mod html;
pub mod objects;
pub mod pages;
pub mod util;

// Modules not needed by tests are left private. The dead_code allow is required because
// these modules contain binary-only code that is unreachable from the library crate root,
// but they are needed to satisfy transitive compile-time dependencies (e.g. api::setlang
// references generated).
#[allow(dead_code)]
mod debug;
#[allow(dead_code)]
mod generated;
#[allow(dead_code)]
mod notify;

include!(concat!(env!("OUT_DIR"), "/icons.rs"));
