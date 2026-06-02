#[compute]
#version 450

// WorldGen10 Slice-3 #1 RISK SPIKE: GPU multiple-flow-direction (MFD) flow accumulation
// by ITERATIVE RELAXATION. This is a MEASUREMENT prototype, NOT wired into the render path.
//
// The CPU reference (array_ops.rs::flow_accumulation_mfd /
// geography_skeleton.py::_flow_accumulation_mfd) is a SEQUENTIAL sorted high->low sweep:
// it processes cells in descending-height order and pushes each cell's accumulated flow
// downhill, so by the time a cell is processed all upstream contributions are already in.
// That data dependency cannot be expressed in one GPU dispatch.
//
// GPU reformulation (PULL relaxation): acc[c] = 1 + sum over the 8 neighbours n that are
// HIGHER than c of  acc_prev[n] * w(n->c).  w(n->c) is the SAME MFD weight the CPU uses,
// computed from n's point of view: among n's strictly-downhill neighbours d, weight to d is
// (drop_n_d / dist)^power, normalized by n's total downhill weight (+1e-12).  Each dispatch
// is one relaxation step reading acc_prev and writing acc_next (ping-pong). After K steps
// the value at a cell equals the exact push-sweep result for every upstream path of length
// <= K cells, so K must reach the longest monotone descending path in the page. As K grows
// the GPU result converges to the CPU sorted-sweep result (this is an APPROXIMATION; the
// gate is cost + convergence, not bit-parity).
//
// One invocation = one cell, one relaxation step. Pure f32. No host sync inside the loop:
// the Rust side ping-pongs the two buffers across K back-to-back dispatches with a
// compute barrier between them, and times the whole loop with RenderingDevice timestamps.

layout(local_size_x = 16, local_size_y = 16) in;

// binding 0: height field (read-only), row-major f32, length n = dim*dim
layout(set = 0, binding = 0, std430) restrict readonly buffer Height { float h[]; } height;
// binding 1: acc_prev (read), binding 2: acc_next (write). Rust swaps which RID is bound
// where between iterations via two alternating uniform sets.
layout(set = 0, binding = 1, std430) restrict readonly  buffer AccPrev { float a[]; } acc_prev;
layout(set = 0, binding = 2, std430) restrict writeonly buffer AccNext { float a[]; } acc_next;

layout(push_constant, std430) uniform Params {
    int dim;        // grid is dim x dim (256)
    float power;    // MFD exponent (CPU default 1.45)
    int pad0;
    int pad1;
} P;

// EXACT (dy, dx, dist) 8-neighbour table from array_ops.rs / geography_skeleton.py.
// Diagonal distance is the literal 1.41421356237 the reference uses (NOT sqrt(2.0)).
const ivec2 NB[8] = ivec2[8](
    ivec2(-1, -1), ivec2(-1, 0), ivec2(-1, 1),
    ivec2( 0, -1),               ivec2( 0, 1),
    ivec2( 1, -1), ivec2( 1, 0), ivec2( 1, 1)
);
const float DIST[8] = float[8](
    1.41421356237, 1.0, 1.41421356237,
    1.0,                1.0,
    1.41421356237, 1.0, 1.41421356237
);

int cell_idx(int x, int y, int dim) { return y * dim + x; }

void main() {
    int dim = P.dim;
    int cx = int(gl_GlobalInvocationID.x);
    int cy = int(gl_GlobalInvocationID.y);
    if (cx >= dim || cy >= dim) return;
    int ci = cell_idx(cx, cy, dim);

    float p = P.power;
    float hc = height.h[ci];

    // Each cell always contributes its own unit (matches acc init = 1.0 on the CPU).
    float acc = 1.0;

    // PULL: gather from neighbours n that are HIGHER than this cell c (so c is downhill of n).
    for (int k = 0; k < 8; ++k) {
        int nx = cx + NB[k].x;
        int ny = cy + NB[k].y;
        if (nx < 0 || nx >= dim || ny < 0 || ny >= dim) continue;
        int ni = cell_idx(nx, ny, dim);
        float hn = height.h[ni];

        // c must be strictly downhill of n for n to send flow to c.
        // Reciprocity of the table: the offset from n to c is -NB[k], and its dist == DIST[k].
        float drop_nc = (hn - hc) / DIST[k];
        if (drop_nc <= 0.0) continue;       // c is not below n -> n sends nothing to c
        float w_nc = pow(drop_nc, p);

        // Recompute n's TOTAL downhill weight so we know n's normalization (the CPU divides
        // by total + 1e-12). We sum over all 8 neighbours d of n that are strictly below n.
        float total_n = 0.0;
        for (int j = 0; j < 8; ++j) {
            int dx = nx + NB[j].x;
            int dy = ny + NB[j].y;
            if (dx < 0 || dx >= dim || dy < 0 || dy >= dim) continue;
            float hd = height.h[cell_idx(dx, dy, dim)];
            float drop_nd = (hn - hd) / DIST[j];
            if (drop_nd > 0.0) total_n += pow(drop_nd, p);
        }

        // n distributes acc_prev[n] across its downhill neighbours in proportion to weight;
        // c's share is w_nc / (total_n + 1e-12). (total_n >= w_nc > 0 here, so safe.)
        acc += acc_prev.a[ni] * (w_nc / (total_n + 1e-12));
    }

    acc_next.a[ci] = acc;
}
