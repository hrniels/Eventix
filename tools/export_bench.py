#!/usr/bin/env python3

# Copyright (C) 2026 Nils Asmussen
#
# SPDX-License-Identifier: GPL-3.0-or-later

import argparse
import json
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--input-dir",
        required=True,
        type=Path,
        help="Criterion benchmark group directory under target/criterion",
    )
    parser.add_argument(
        "--output",
        required=True,
        type=Path,
        help="Output JSON file in github-action-benchmark custom format",
    )
    return parser.parse_args()


def collect_benchmarks(root: Path) -> list[dict[str, object]]:
    benchmarks: list[dict[str, object]] = []
    for estimate_path in sorted(root.rglob("new/estimates.json")):
        benchmark_path = estimate_path.with_name("benchmark.json")
        if not benchmark_path.is_file():
            continue

        with benchmark_path.open(encoding="utf-8") as f:
            benchmark = json.load(f)
        with estimate_path.open(encoding="utf-8") as f:
            estimates = json.load(f)

        benchmarks.append(
            {
                "name": benchmark["title"],
                "unit": "ns",
                "value": estimates["mean"]["point_estimate"],
            }
        )

    benchmarks.sort(key=lambda bench: str(bench["name"]))
    return benchmarks


def main() -> int:
    args = parse_args()
    benchmarks = collect_benchmarks(args.input_dir)
    if not benchmarks:
        raise SystemExit(f"no criterion benchmarks found in {args.input_dir}")

    args.output.write_text(json.dumps(benchmarks, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
