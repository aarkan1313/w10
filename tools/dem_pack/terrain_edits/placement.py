from __future__ import annotations
from dataclasses import dataclass
import heapq
import numpy as np


@dataclass(frozen=True)
class LowCorridorParams:
    low_pref: float = 8.0       # how hard the route prefers LOW ground (threads valleys); higher = more valley
    route_count: int = 1        # sparse by default (one sweeping crossing per axis); >1 = more passes


def low_corridor_route(height: np.ndarray, ctx, p: LowCorridorParams, axis: str = "x", start_row: int | None = None) -> list[tuple[int, int]]:
    """Deterministic least-cost crossing biased HARD to low ground -> threads the natural valleys edge-to-edge.
    axis='x' west->east, 'z' north->south. cost = step_len * (1 + low_pref * normalized_height(target))."""
    if axis not in ("x", "z"):
        raise ValueError("axis must be 'x' or 'z'")
    H = np.asarray(height, dtype=np.float64)
    work = H if axis == "x" else H.T
    R, C = work.shape
    Hn = (work - work.min()) / (np.ptp(work) + 1e-9)
    dist = np.full(R * C, np.inf)
    prev = np.full(R * C, -1, dtype=np.int64)
    pq: list[tuple[float, int]] = []
    rows = [start_row] if start_row is not None else range(R)
    for r in rows:
        dist[r * C] = 0.0
        heapq.heappush(pq, (0.0, r * C))
    target = -1
    nbrs = ((-1, 0, 1.0), (1, 0, 1.0), (0, 1, 1.0), (0, -1, 1.0), (-1, 1, 1.41421356), (1, 1, 1.41421356), (-1, -1, 1.41421356), (1, -1, 1.41421356))
    while pq:
        d, idx = heapq.heappop(pq)
        if d > dist[idx]:
            continue
        r, c = divmod(idx, C)
        if c == C - 1:
            target = idx
            break
        for dr, dc, L in nbrs:
            nr, nc = r + dr, c + dc
            if not (0 <= nr < R and 0 <= nc < C):
                continue
            cost = L * (1.0 + float(p.low_pref) * Hn[nr, nc])
            nidx = nr * C + nc
            nd = d + cost
            if nd < dist[nidx]:
                dist[nidx] = nd
                prev[nidx] = idx
                heapq.heappush(pq, (nd, nidx))
    path = []
    node = target
    while node != -1:
        r, c = divmod(node, C)
        path.append((r, c) if axis == "x" else (c, r))
        node = int(prev[node])
    path.reverse()
    return path


@dataclass(frozen=True)
class ContourSweepParams:
    low_pref: float = 12.0    # stronger valley bias -> longer sweeping traverses


def contour_sweep(height, ctx, p: ContourSweepParams, axis: str = "x", start_row: int | None = None):
    """Sweeping valley-following crossing (Fellowship look). For now a strong-valley-bias low_corridor_route;
    a true contour traversal is a later refinement (spec 3.1). Same return shape."""
    return low_corridor_route(height, ctx, LowCorridorParams(low_pref=p.low_pref), axis=axis, start_row=start_row)
