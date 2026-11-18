// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::hint::black_box;
use std::path::Path;
use std::sync::Arc;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use eventix_ical::col::CalDir;

const CAL_ID: &str = "bench-cal";
const FIXTURE_COUNT: u64 = 1_000;

/// Expands the same ten seed calendars to keep historical benchmark results comparable.
fn build_calendar_dir() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::TempDir::new().expect("create tempdir");
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).expect("create calendar dir");
    let mut seeds = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/benches/data"))
        .read_dir()
        .expect("read fixture dir")
        .map(|entry| entry.expect("read fixture entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("ics"))
        .collect::<Vec<_>>();
    seeds.sort();
    assert_eq!(seeds.len(), 10, "expected ten seed ICS fixtures");
    for index in 0..FIXTURE_COUNT as usize {
        let seed = &seeds[index % seeds.len()];
        let content = std::fs::read_to_string(seed).expect("read seed fixture");
        let stem = seed
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("fixture stem");
        let unique = format!("{stem}-{index:04}");
        let content = content.replace("UID:", &format!("UID:{unique}-"));
        std::fs::write(cal_dir.join(format!("{unique}.ics")), content).expect("write fixture");
    }
    (tmp, cal_dir)
}

fn benchmark_parse(c: &mut Criterion) {
    let (_tmp, cal_dir) = build_calendar_dir();

    let mut group = c.benchmark_group("ics/parse_directory");
    group.throughput(Throughput::Elements(FIXTURE_COUNT));
    group.bench_function("generated_calendar_dir", |b| {
        b.iter(|| {
            let parsed = CalDir::new_from_dir(
                Arc::new(CAL_ID.to_string()),
                cal_dir.clone(),
                "Benchmark Calendar".to_string(),
                false,
            )
            .expect("parse generated benchmark calendar dir");
            black_box(parsed);
        });
    });
    group.finish();
}

criterion_group!(parse_benchmarks, benchmark_parse);
criterion_main!(parse_benchmarks);
