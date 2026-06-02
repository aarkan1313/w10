#[compute]
#version 450

// WorldGen10 Task 4a.3 probe: a 1x1x1 compute shader that evaluates ONE noise/warp
// primitive at one coord and writes one float to a storage buffer at binding 0.
//
// This file is the `main` half. The Rust probe (primitive_probe.rs) PREPENDS
// recipe_primitives.glsl (the helpers + i64-emulated hash) before this, because Godot
// GLSL has no #include. The `#[compute]` header line and any other non-GLSL `#[...]`
// lines are stripped by the Rust side (same as flow_spike) before compilation.
//
// The push constant carries an int `fn_sel` and the primitive args as floats:
//   fn_sel: 0=hash2 1=value_noise 2=fbm 3=ridged_multifractal 4=warp_x 5=warp_z
//   a0..a4: coords / freq / seed (cast to int where the primitive needs an int).
// Probe convention for the param-shaped primitives:
//   hash2(ix=int(a0), iz=int(a1), seed=int(a2))
//   value_noise(wx=a0, wz=a1, seed=int(a2))
//   fbm(wx=a0, wz=a1, base_freq=a2, octaves=4, seed=int(a3), gain=0.5, lacunarity=2.0)
//   ridged_multifractal(wx=a0, wz=a1, base_freq=a2, octaves=5, seed=int(a3),
//                       gain=0.5, lacunarity=2.0, offset=1.0, weight_gain=1.35)
//   warp_x/warp_z(wx=a0, wz=a1, warp_amount=a2, warp_freq=a3, seed=int(a4))
// These octave/param choices MUST match export_primitive_parity_fixture.py exactly.

layout(local_size_x = 1, local_size_y = 1, local_size_z = 1) in;

layout(set = 0, binding = 0, std430) restrict writeonly buffer Out { float value; } outbuf;

layout(push_constant, std430) uniform Params {
    int fn_sel;
    int pad0;
    int pad1;
    int pad2;
    float a0;
    float a1;
    float a2;
    float a3;
    float a4;
    float pad3;
    float pad4;
    float pad5;
} P;

void main() {
    float r = 0.0;
    if (P.fn_sel == 0) {
        r = hash2(int(P.a0), int(P.a1), int(P.a2));
    } else if (P.fn_sel == 1) {
        r = value_noise(P.a0, P.a1, int(P.a2));
    } else if (P.fn_sel == 2) {
        r = fbm(P.a0, P.a1, P.a2, 4, int(P.a3), 0.5, 2.0);
    } else if (P.fn_sel == 3) {
        r = ridged_multifractal(P.a0, P.a1, P.a2, 5, int(P.a3), 0.5, 2.0, 1.0, 1.35);
    } else if (P.fn_sel == 4) {
        vec2 w = recursive_domain_warp(P.a0, P.a1, P.a2, P.a3, int(P.a4));
        r = w.x;
    } else if (P.fn_sel == 5) {
        vec2 w = recursive_domain_warp(P.a0, P.a1, P.a2, P.a3, int(P.a4));
        r = w.y;
    }
    outbuf.value = r;
}
