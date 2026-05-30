shader_type spatial;
render_mode unshaded, cull_disabled;

// Explicit sampler state: linear filter, CLAMP-to-edge, no repeat. The page covers
// [origin, origin+span] with texel 0 at the corner (texel-corner gen); clamp_to_edge means a
// vertex exactly on the boundary (uv=0 or 1) reads the edge texel with NO wrap/border bleed, so
// abutting pages return the identical boundary value -> the shared edge joins.
uniform sampler2D height_tex : filter_linear, repeat_disable;        // this level's R32F page (pool Texture2DRD)
uniform sampler2D coarse_height_tex : filter_linear, repeat_disable; // next-coarser level's page (for the morph)
uniform float world_span = 8192.0;        // this level's page world span
uniform float coarse_span = 8192.0;       // the coarser level's page world span
uniform vec2 page_origin = vec2(0.0);     // this fine page's WORLD lower-XZ corner; page covers [origin, origin+world_span]
uniform vec2 coarse_origin = vec2(0.0);   // the coarser page's WORLD lower-XZ corner; covers [origin, origin+coarse_span]
uniform vec2 level_center = vec2(0.0);    // WORLD center of this level's 3x3 tile neighborhood
uniform float level_half_extent = 12288.0;// 1.5 * world_span (half the 3x3 neighborhood width)
uniform float height_scale = 1.0;         // visual amplitude (config; 1.0 = raw metres)
uniform float morph_region = 0.0;         // transition width as a fraction of the half-extent (0 = no morph)
uniform float relief_ref = 2000.0;        // color gradient normalization

varying float v_render_height;     // DISPLAYED height (base + M5 detail); NOT the facts/collision height
varying float v_morph;             // debug: the geomorph factor t (0 fine .. 1 coarse) per vertex
// GLOBAL debug mode (one RenderingServer.global_shader_parameter for the whole scene, so a key in
// the review scene flips it with no per-tile material access / no Rust rebuild):
// 0 = normal height color · 1 = morph-band heatmap (blue fine / green blend / red coarse).
global uniform float wg_dbg_mode;
// M5 detail: GLOBAL amplitude knob (one RenderingServer.global_shader_parameter for the
// whole scene, set by the harness / gate). 0.0 = no detail (byte-identical to pre-M5).
// Detail is added to VERTEX.y AFTER h_base is formed, so the base/collision height is
// untouched (facts read h_base). See spec §4 invariant 1.
global uniform float wg_detail_amp;

// ---- M5 detail: value-noise fBm, pure function of world XZ (edge-safe by construction) ----
// Integer hash (no sin-banding): 2D -> [0,1). Cheap, no texture, no streaming.
float wg_hash2(vec2 p) {
	// fract-of-large-product hash with two mixing steps; stable + bandless enough for detail.
	vec3 p3 = fract(vec3(p.xyx) * 0.1031);
	p3 += dot(p3, p3.yzx + 33.33);
	return fract((p3.x + p3.y) * p3.z);
}

// value noise: bilinear blend of 4 lattice hashes with a smoothstep (cubic, C1) weight.
float wg_value_noise(vec2 x) {
	vec2 i = floor(x);
	vec2 f = fract(x);
	float a = wg_hash2(i);
	float b = wg_hash2(i + vec2(1.0, 0.0));
	float c = wg_hash2(i + vec2(0.0, 1.0));
	float d = wg_hash2(i + vec2(1.0, 1.0));
	vec2 u = f * f * (3.0 - 2.0 * f);
	float n = mix(mix(a, b, u.x), mix(c, d, u.x), u.y);  // in [0,1]
	// Quality choice: the bilinear blend of 4 hashes clusters toward 0.5 (central tendency), so
	// raw value noise looks washed-out/muddy. smoothstep pushes the distribution toward the
	// extremes, giving more believable relief per metre of wg_detail_amp. It is monotonic
	// [0,1]->[0,1], so the output stays in [0,1] and the |fbm|<=1 bound is preserved exactly.
	return smoothstep(0.0, 1.0, n);  // in [0,1], higher contrast
}

// fBm: DETAIL_OCTAVES of value noise, centered to [-1,1]*ceiling. Fixed octaves/gain for
// Slice 1 (they become config in Slice 4). Returns a value whose |.| <= 1.0 (the sum of a
// geometric series with gain 0.5 over 5 octaves, normalized) so wg_detail_amp is the metre
// ceiling directly.
const int   WG_DETAIL_OCTAVES = 5;
const float WG_DETAIL_GAIN    = 0.5;
const float WG_DETAIL_FREQ    = 0.00025;  // ~1/4000 m base wavelength. LOWERED from 0.0009 so the 5
                                          // octaves (~4000/2000/1000/500/250 m) stay at/above the
                                          // GRID_RES=64 fly mesh Nyquist (256 m) — at 0.0009 the 3
                                          // finest octaves (278/139/69 m) were below the 128 m vertex
                                          // spacing and aliased AWAY, leaving only faint km-scale swell
                                          // (the "no visible detail" fly finding). All octaves now
                                          // resolve on the shipped mesh. (Final freq is an S4 config knob.)
float wg_fbm_detail(vec2 world_xz) {
	float sum = 0.0;
	float amp = 1.0;
	float norm = 0.0;
	float freq = WG_DETAIL_FREQ;
	for (int i = 0; i < WG_DETAIL_OCTAVES; i++) {
		sum  += amp * (wg_value_noise(world_xz * freq) * 2.0 - 1.0); // center to [-1,1]
		norm += amp;
		freq *= 2.0;
		amp  *= WG_DETAIL_GAIN;
	}
	return sum / max(norm, 1e-6);   // normalized to [-1,1]
}

// World XZ of this vertex = mesh-local XZ + the instance origin (rings recenter by
// translating the instance). MODEL_MATRIX maps local -> world.
void vertex() {
	vec3 world = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;

	// Fine sample by TRUE WORLD UV, matching the page's texel-corner generation: uv=0 -> texel 0
	// (= page_origin), uv=1 -> texel N-1 (= page_origin + world_span). At a tile seam, BOTH
	// abutting tiles map their shared-edge vertex onto the SAME shared boundary texel -> no seam.
	// (Was VERTEX.xz/span + 0.5, which mapped edge vertices onto the texture BORDER, a half-texel
	// off the texel centers, and abutting tiles read opposite-offset borders -> a visible wall.)
	vec2 uv_fine = (world.xz - page_origin) / world_span;
	float h_fine = texture(height_tex, uv_fine).r;

	// Geomorph: blend to the coarser level ONLY at this LEVEL's true outer ring (the outward
	// edge of the 3x3 neighborhood), measured from the NEIGHBORHOOD CENTER — not the tile center.
	// Tile-local distance fired the morph at every one of the 9 tiles' edges (an interior lattice
	// that swept under motion). Chebyshev (max-of-axes) so the band is a square matching the ring;
	// cheb in [0,1], 1 at the neighborhood outer edge.
	float cheb = max(abs(world.x - level_center.x), abs(world.z - level_center.y)) / level_half_extent;
	float region = max(morph_region, 1e-6);
	float t = clamp((cheb - (1.0 - region)) / region, 0.0, 1.0); // 0 until the inner edge of the region

	// Coarser page sampled corner-relative in world (same texel-corner convention).
	vec2 uv_coarse = (world.xz - coarse_origin) / coarse_span;
	float h_coarse = texture(coarse_height_tex, uv_coarse).r;

	float h = mix(h_fine, h_coarse, t);   // h_base: THE facts/collision height (untouched)
	v_morph = t;
	// M5 detail: bounded world-space fBm added ON TOP of the base height. Pure function of
	// world.xz -> abutting tiles agree on shared edges (edge-safe). h (above) is unchanged,
	// so facts/collision parity is preserved. Slice 1 = flat amp (no modulation/fade yet).
	float detail = wg_fbm_detail(world.xz) * wg_detail_amp;
	// v_render_height is the DISPLAYED (debug/height-color) height = base + detail. It is NOT the
	// facts/collision height: collision reads h_base via the separate CPU facts path, so the
	// parity contract is on h (unchanged above), not on this varying.
	v_render_height = h + detail;
	VERTEX.y = (h + detail) * height_scale;
}

void fragment() {
	if (wg_dbg_mode > 0.5) {
		// morph-band heatmap: blue = pure fine (t=0), red = pure coarse (t=1), green = the blend
		// in between. A hard LOD pop shows as an abrupt blue->red edge with NO green; a proper
		// geomorph shows a WIDE green gradient where fine eases into coarse.
		ALBEDO = vec3(v_morph, 1.0 - abs(v_morph - 0.5) * 2.0, 1.0 - v_morph);
	} else {
		float t = clamp(v_render_height / relief_ref * 0.5 + 0.5, 0.0, 1.0);
		ALBEDO = vec3(t, t * 0.8 + 0.1, 1.0 - t);
	}
}
