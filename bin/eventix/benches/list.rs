// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod common;

use axum::Router;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};

use crate::common::{CAL_ID, FIXTURE_COUNT, build_runtime, build_state, run_request};

fn make_router(state: eventix_state::EventixState) -> Router {
    Router::new().nest("/pages/list", eventix::pages::list::router(state))
}

fn benchmark_list(c: &mut Criterion) {
    let runtime = build_runtime();
    let (state, _tmp) = build_state();

    let cases = [
        (
            "all_items",
            format!("/pages/list/results/content?dirs%5B%5D={CAL_ID}&page=1&conjunction=And"),
        ),
        (
            "keyword_or",
            format!(
                "/pages/list/results/content?dirs%5B%5D={CAL_ID}&page=1&conjunction=Or&keywords=timezone%20berlin"
            ),
        ),
        (
            "keyword_and",
            format!(
                "/pages/list/results/content?dirs%5B%5D={CAL_ID}&page=1&conjunction=And&keywords=project%20review"
            ),
        ),
    ];

    let mut group = c.benchmark_group("pages/list/results/content");
    group.throughput(Throughput::Elements(FIXTURE_COUNT));

    for (name, uri) in cases {
        group.bench_function(name, |b| {
            b.to_async(&runtime).iter(|| async {
                let (status, body) = run_request(make_router(state.clone()), &uri).await;
                assert_eq!(
                    status,
                    axum::http::StatusCode::OK,
                    "unexpected status for {uri}"
                );
                assert!(
                    body.contains("Events and Tasks"),
                    "missing list heading for {uri}"
                );
            });
        });
    }

    group.finish();
}

criterion_group!(list_results, benchmark_list);
criterion_main!(list_results);
