// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod add;
pub mod cancel;
pub mod complete;
pub mod delete;
pub mod details;
pub mod editalarm;
pub mod occlist;
pub mod respond;
pub mod shift;
pub mod toggle;

use axum::Router;
use eventix_state::EventixState;

pub fn router(state: EventixState) -> Router {
    Router::new()
        .with_state(state.clone())
        .merge(add::router(state.clone()))
        .merge(cancel::router(state.clone()))
        .merge(complete::router(state.clone()))
        .merge(delete::router(state.clone()))
        .merge(details::router(state.clone()))
        .merge(editalarm::router(state.clone()))
        .merge(shift::router(state.clone()))
        .merge(occlist::router(state.clone()))
        .merge(respond::router(state.clone()))
        .merge(toggle::router(state.clone()))
}
