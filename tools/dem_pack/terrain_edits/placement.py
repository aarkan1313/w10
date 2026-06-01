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


@dataclass(frozen=True)
class CrossWaypointParams:
    low_pref: float = 8.0          # valley bias for the arms
    center_frac: float = 0.5       # half-width of the central region the meeting waypoint is chosen from
                                   # (0.5 = the middle 50% of the field); the lowest cell in it = the meeting point


def _route_point_to_edge(height, p_low_pref, src, which):
    """Least-cost path from a single source cell to the nearest cell on edge `which` (W/E/N/S), valley-biased.
    Used to build arms that all share the meeting waypoint (src)."""
    H = np.asarray(height, dtype=np.float64)
    R, C = H.shape
    Hn = (H - H.min()) / (np.ptp(H) + 1e-9)
    dist = np.full(R * C, np.inf)
    prev = np.full(R * C, -1, dtype=np.int64)
    si = int(src[0]) * C + int(src[1])
    dist[si] = 0.0
    pq = [(0.0, si)]
    nbrs = ((-1, 0, 1.0), (1, 0, 1.0), (0, 1, 1.0), (0, -1, 1.0), (-1, 1, 1.41421356), (1, 1, 1.41421356), (-1, -1, 1.41421356), (1, -1, 1.41421356))
    target = -1
    while pq:
        d, idx = heapq.heappop(pq)
        if d > dist[idx]:
            continue
        r, c = divmod(idx, C)
        if (which == "W" and c == 0) or (which == "E" and c == C - 1) or (which == "N" and r == 0) or (which == "S" and r == R - 1):
            target = idx
            break
        for dr, dc, L in nbrs:
            nr, nc = r + dr, c + dc
            if not (0 <= nr < R and 0 <= nc < C):
                continue
            cost = L * (1.0 + float(p_low_pref) * Hn[nr, nc])
            nidx = nr * C + nc
            nd = d + cost
            if nd < dist[nidx]:
                dist[nidx] = nd
                prev[nidx] = idx
                heapq.heappush(pq, (nd, nidx))
    path = []
    node = target
    while node != -1:
        path.append(divmod(node, C))
        node = int(prev[node])
    return path  # waypoint -> edge


def cross_waypoint(height, ctx, p: CrossWaypointParams, axis: str = "x", start_row: int | None = None):
    """FULL-TRAVERSAL guarantee: route 4 arms from a central meeting waypoint (the lowest cell in the middle
    center_frac of the field) out to each edge (W/E/N/S). All arms share the waypoint, so the union is ONE
    connected network that reaches all four edges -> you can traverse fully left<->right AND up<->down and get
    between them ("meeting in the middle"). Returns a LIST OF ROUTES (the 4 arms); apply_edits profiles each.
    `axis`/`start_row` are ignored (this placement defines its own geometry)."""
    H = np.asarray(height, dtype=np.float64)
    R, C = H.shape
    f = float(p.center_frac)
    r0, r1 = int((0.5 - f / 2) * R), int((0.5 + f / 2) * R)
    c0, c1 = int((0.5 - f / 2) * C), int((0.5 + f / 2) * C)
    sub = H[r0:r1, c0:c1]
    wr, wc = np.unravel_index(int(np.argmin(sub)), sub.shape)
    waypoint = (int(wr) + r0, int(wc) + c0)
    return [_route_point_to_edge(H, p.low_pref, waypoint, w) for w in ("W", "E", "N", "S")]
