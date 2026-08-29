// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::{
    Router,
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
};
use eventix_state::EventixState;
use futures::{StreamExt, stream};
use std::{convert::Infallible, time::Duration};
use tokio::sync::broadcast::error::RecvError;
use tracing::debug;

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/events", get(handler))
        .with_state(state)
}

async fn handler(
    State(state): State<EventixState>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let changes = state.lock().await.subscribe_external_changes();
    let changes = stream::unfold(changes, |mut changes| async move {
        match changes.recv().await {
            Ok(()) | Err(RecvError::Lagged(_)) => {
                debug!("Sending external-change event to client");
                Some((
                    Ok(Event::default().event("external-change").data("changed")),
                    changes,
                ))
            }
            Err(RecvError::Closed) => None,
        }
    });

    // Write a body chunk immediately so the browser treats this as an active SSE stream.
    let events = stream::once(async { Ok(Event::default().comment("connected")) }).chain(changes);
    Sse::new(events).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}
