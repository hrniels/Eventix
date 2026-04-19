// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod common;

use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use eventix_ical::col::CalDir;

use crate::common::{CAL_ID, FIXTURE_COUNT, benchmark_local_tz, build_calendar_dir};

fn benchmark_parse(c: &mut Criterion) {
    let local_tz = benchmark_local_tz();
    let (_tmp, cal_dir) = build_calendar_dir();

    let mut group = c.benchmark_group("ics/parse_directory");
    group.throughput(Throughput::Elements(FIXTURE_COUNT));
    group.bench_function("generated_calendar_dir", |b| {
        b.iter(|| {
            let parsed = CalDir::new_from_dir(
                Arc::new(CAL_ID.to_string()),
                cal_dir.clone(),
                "Benchmark Calendar".to_string(),
                &local_tz,
            )
            .expect("parse generated benchmark calendar dir");
            black_box(parsed);
        });
    });
    group.finish();
}

criterion_group!(parse_benchmarks, benchmark_parse);
criterion_main!(parse_benchmarks);
