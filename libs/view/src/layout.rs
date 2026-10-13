// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

/// Assigns rows to half-open day ranges, returning placements in input order.
///
/// Adjacent spans share a row. Earlier starts are placed first, with longer spans first at
/// equal starts. Input order breaks remaining ties, so callers should supply a stable order.
pub fn rows(intervals: &[(usize, usize)]) -> Vec<usize> {
    let mut order: Vec<_> = (0..intervals.len()).collect();
    order.sort_by_key(|&i| (intervals[i].0, std::cmp::Reverse(intervals[i].1), i));
    let mut ends = Vec::new();
    let mut result = vec![0; intervals.len()];
    for i in order {
        let (start, end) = intervals[i];
        let row = ends
            .iter()
            .position(|&end| end <= start)
            .unwrap_or(ends.len());
        if row == ends.len() {
            ends.push(end);
        } else {
            ends[row] = end;
        }
        result[i] = row;
    }
    result
}

/// Horizontal placement within a group of overlapping events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Columns {
    pub count: usize,
    pub offset: usize,
    pub span: usize,
}

/// Assigns columns to half-open intervals, returning placements in input order.
///
/// Adjacent intervals share a column. Empty intervals occupy one unit. Events expand rightward
/// into columns that contain no conflicting interval. Input need not be sorted.
pub fn columns(intervals: &[(i64, i64)]) -> Vec<Columns> {
    let intervals: Vec<_> = intervals.iter().map(|&(s, e)| (s, e.max(s + 1))).collect();
    let mut order: Vec<_> = (0..intervals.len()).collect();
    order.sort_by_key(|&i| (intervals[i].0, i));
    let mut result = vec![Columns::default(); intervals.len()];
    let mut group = Vec::new();
    let mut end = i64::MIN;
    for i in order {
        if intervals[i].0 >= end && !group.is_empty() {
            place_group(&group, &intervals, &mut result);
            group.clear();
        }
        if group.is_empty() {
            end = intervals[i].1;
        } else {
            end = end.max(intervals[i].1);
        }
        group.push(i);
    }
    place_group(&group, &intervals, &mut result);
    result
}

fn place_group(group: &[usize], intervals: &[(i64, i64)], result: &mut [Columns]) {
    let overlaps =
        |a: usize, b: usize| intervals[a].0 < intervals[b].1 && intervals[b].0 < intervals[a].1;
    let mut slots: Vec<Vec<usize>> = Vec::new();
    for &i in group {
        let slot = slots
            .iter()
            .position(|s| s.iter().all(|&j| !overlaps(i, j)));
        match slot {
            Some(s) => slots[s].push(i),
            None => slots.push(vec![i]),
        }
    }
    for (offset, slot) in slots.iter().enumerate() {
        for &i in slot {
            let span = 1 + slots[offset + 1..]
                .iter()
                .take_while(|s| s.iter().all(|&j| !overlaps(i, j)))
                .count();
            result[i] = Columns {
                count: slots.len(),
                offset,
                span,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{columns, rows};

    #[test]
    fn span_rows_reuse_adjacency_and_prefer_longer_spans() {
        assert_eq!(
            rows(&[(2, 4), (0, 2), (0, 5), (4, 7), (1, 3)]),
            [1, 1, 0, 1, 2]
        );
        assert!(rows(&[]).is_empty());
    }

    #[test]
    fn adjacency_and_unsorted_bridge() {
        let c = columns(&[(20, 30), (0, 10), (5, 25), (30, 40)]);
        assert_eq!(c.iter().map(|c| c.count).collect::<Vec<_>>(), [2, 2, 2, 1]);
        assert_eq!(c[0].offset, c[1].offset);
        assert_ne!(c[1].offset, c[2].offset);
    }

    #[test]
    fn expands_across_free_columns_and_handles_points() {
        let c = columns(&[(0, 100), (0, 20), (0, 10), (20, 30), (100, 100)]);
        assert_eq!(c[3].span, 2);
        assert_eq!(c[3].count, 3);
        assert_eq!(c[4].count, 1);
        assert!(columns(&[]).is_empty());
    }
}
