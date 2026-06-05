//! Deterministic Dijkstra over a 4-connected grid, ported bit-faithfully from
//! traverse_corridor._dijkstra_cost_field (py:95-130): (cost, flattened-index) min-heap
//! tie-break (lower index wins on equal cost — LOAD-BEARING, ~40k ties on the real field),
//! fixed neighbour order (-1,0),(1,0),(0,1),(0,-1), stop at first popped target.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use super::cost::step_cost;
use super::TraverseParams;

// Python heapq is a MIN-heap of (cost, idx). Rust BinaryHeap is a MAX-heap, so invert Ord:
// the entry with the SMALLEST (cost, idx) must compare GREATEST so it pops first.
#[derive(Copy, Clone)]
struct Node {
    cost: f64,
    idx: usize,
}
impl PartialEq for Node {
    fn eq(&self, o: &Self) -> bool {
        self.cost == o.cost && self.idx == o.idx
    }
}
impl Eq for Node {}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> Ordering {
        // reverse cost, then reverse idx -> smallest (cost, idx) is "greatest" -> popped first.
        // costs here are finite & non-NaN (step_cost returns finite positives).
        o.cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
            .then(o.idx.cmp(&self.idx))
    }
}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// Returns (prev, dist, target_idx_or_-1). 4-connected; fixed neighbour order.
#[allow(clippy::too_many_arguments)]
pub fn dijkstra_cost_field<F: Fn(usize, usize) -> bool>(
    slope: &[f64],
    h: &[f64],
    chan: &[f64],
    rows: usize,
    cols: usize,
    cell_m: f64,
    p: &TraverseParams,
    sources: &[(usize, usize)],
    is_target: F,
) -> (Vec<i64>, Vec<f64>, i64) {
    let mut dist = vec![f64::INFINITY; rows * cols];
    let mut prev = vec![-1_i64; rows * cols];
    let mut pq: BinaryHeap<Node> = BinaryHeap::new();
    for &(r, c) in sources {
        let idx = r * cols + c;
        let cost = step_cost(slope[idx], h[idx], chan[idx], cell_m, p);
        if cost < dist[idx] {
            dist[idx] = cost;
            pq.push(Node { cost, idx });
        }
    }
    let neighbours: [(i64, i64); 4] = [(-1, 0), (1, 0), (0, 1), (0, -1)];
    let mut target: i64 = -1;
    while let Some(Node { cost: d, idx }) = pq.pop() {
        if d > dist[idx] {
            continue;
        }
        let r = idx / cols;
        let c = idx % cols;
        if is_target(r, c) {
            target = idx as i64;
            break;
        }
        for (dr, dc) in neighbours {
            let nr = r as i64 + dr;
            let nc = c as i64 + dc;
            if nr < 0 || nr >= rows as i64 || nc < 0 || nc >= cols as i64 {
                continue;
            }
            let nidx = nr as usize * cols + nc as usize;
            let nd = d + step_cost(slope[nidx], h[nidx], chan[nidx], cell_m, p);
            if nd < dist[nidx] {
                dist[nidx] = nd;
                prev[nidx] = idx as i64;
                pq.push(Node { cost: nd, idx: nidx });
            }
        }
    }
    (prev, dist, target)
}

/// Mirror of traverse_corridor._reconstruct_path (py:133-140).
pub fn reconstruct_path(prev: &[i64], target: usize, cols: usize) -> Vec<(usize, usize)> {
    let mut path: Vec<(usize, usize)> = Vec::new();
    let mut node: i64 = target as i64;
    while node != -1 {
        let n = node as usize;
        path.push((n / cols, n % cols));
        node = prev[n];
    }
    path.reverse();
    path
}
