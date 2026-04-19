window.BENCHMARK_DATA = {
  "lastUpdate": 1776615267045,
  "repoUrl": "https://github.com/hrniels/Eventix",
  "entries": {
    "Eventix List Benchmark": [
      {
        "commit": {
          "author": {
            "name": "hrniels",
            "username": "hrniels"
          },
          "committer": {
            "name": "hrniels",
            "username": "hrniels"
          },
          "id": "63de4b7c02441028c2e661485585a00d306b7240",
          "message": "Added benchmarks, also to CI",
          "timestamp": "2026-04-17T18:20:55Z",
          "url": "https://github.com/hrniels/Eventix/pull/24/commits/63de4b7c02441028c2e661485585a00d306b7240"
        },
        "date": 1776615194380,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "pages/list/results/content/all_items",
            "value": 5397383.255912899,
            "unit": "ns"
          },
          {
            "name": "pages/list/results/content/keyword_and",
            "value": 5709133.866737091,
            "unit": "ns"
          },
          {
            "name": "pages/list/results/content/keyword_or",
            "value": 5829052.574453387,
            "unit": "ns"
          }
        ]
      }
    ],
    "Eventix Monthly Benchmark": [
      {
        "commit": {
          "author": {
            "name": "hrniels",
            "username": "hrniels"
          },
          "committer": {
            "name": "hrniels",
            "username": "hrniels"
          },
          "id": "63de4b7c02441028c2e661485585a00d306b7240",
          "message": "Added benchmarks, also to CI",
          "timestamp": "2026-04-17T18:20:55Z",
          "url": "https://github.com/hrniels/Eventix/pull/24/commits/63de4b7c02441028c2e661485585a00d306b7240"
        },
        "date": 1776615266801,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "pages/monthly/content/current_month",
            "value": 6445441.544071093,
            "unit": "ns"
          },
          {
            "name": "pages/monthly/content/dense_month",
            "value": 4773051.676426978,
            "unit": "ns"
          },
          {
            "name": "pages/monthly/content/explicit_month",
            "value": 52970861.1,
            "unit": "ns"
          }
        ]
      }
    ]
  }
}