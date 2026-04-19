// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod common;

use axum::Router;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};

use crate::common::{FIXTURE_COUNT, build_runtime, build_state, run_request};

fn make_router(state: eventix_state::EventixState) -> Router {
    Router::new().nest("/pages/weekly", eventix::pages::weekly::router(state))
}

fn benchmark_weekly(c: &mut Criterion) {
    let runtime = build_runtime();
    let (state, _tmp) = build_state();

    let cases = [
        ("current_week", "/pages/weekly/content".to_string()),
        (
            "explicit_week",
            "/pages/weekly/content?date=2026-02-02".to_string(),
        ),
        (
            "dense_week",
            "/pages/weekly/content?date=2026-06-01".to_string(),
        ),
    ];

    let mut group = c.benchmark_group("pages/weekly/content");
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
                    body.contains("loadWeeklyContent"),
                    "missing weekly content for {uri}"
                );
            });
        });
    }

    group.finish();
}

criterion_group!(weekly_content, benchmark_weekly);
criterion_main!(weekly_content);
