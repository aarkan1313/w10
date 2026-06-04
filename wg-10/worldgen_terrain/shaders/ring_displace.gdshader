shader_type spatial;
render_mode unshaded, cull_disabled;

// Explicit sampler state: linear filter, CLAMP-to-edge, no repeat. The page covers
// [origin, origin+span] with texel 0 at the corner (texel-corner gen); clamp_to_edge means a
// vertex exactly on the boundary (uv=0 or 1) reads the edge texel with NO wrap/border bleed, so
// abutting pages return the identical boundary value -> the shared edge joins.
uniform sampler2D height_tex : filter_linear, repeat_disable;        // this level's R32F page (pool Texture2DRD)
uniform sampler2D coarse_height_tex : filter_linear, repeat_disable; // next-coarser level's page (for the morph)
uniform sampler2D static_material_tex : filter_nearest, repeat_disable; // static-reference material/corridor class codes
uniform float world_span = 8192.0;        // this level's page world span
uniform float coarse_span = 8192.0;       // the coarser level's page world span
uniform vec2 page_origin = vec2(0.0);     // this fine page's WORLD lower-XZ corner; page covers [origin, origin+world_span]
uniform vec2 coarse_origin = vec2(0.0);   // the coarser page's WORLD lower-XZ corner; covers [origin, origin+coarse_span]
uniform vec2 level_center = vec2(0.0);    // WORLD center of this level's 3x3 tile neighborhood
uniform float level_half_extent = 12288.0;// 1.5 * world_span (half the 3x3 neighborhood width)
uniform float relief_scale = 1.0;         // authoritative world-relief multiplier (render side). Same
                                          // value as Wg10Facts.relief_scale -> visible==collision. Was
                                          // `height_scale`; folded into one relief knob (spec §5).
uniform float morph_region = 0.0;         // transition width as a fraction of the half-extent (0 = no morph)
uniform float relief_ref = 2000.0;        // color gradient normalization
uniform float page_fade = 1.0;            // temporal fade for newly resident pages: 0 parent -> 1 fine
uniform vec4 biome_debug_color : source_color = vec4(0.45, 0.45, 0.45, 1.0);
uniform float biome_material_mix = 0.0;   // WORLD-only material tint from page route color.
uniform float static_material_mix = 0.0;  // REFERENCE-only per-texel material/corridor override.

varying float v_render_height;     // DISPLAYED height (base + M5 detail); NOT the facts/collision height
varying float v_morph;             // debug: the geomorph factor t (0 fine .. 1 coarse) per vertex
varying float v_slope;             // displayed rise/run estimate for terrain material selection
varying vec3 v_terrain_normal;      // manual lighting normal derived from the displayed height field
varying float v_detail_norm;        // displayed world-space detail, normalized by wg_detail_amp
varying vec2 v_page_uv;             // fine page UV for static-reference material/corridor texture
// GLOBAL debug mode (one RenderingServer.global_shader_parameter for the whole scene, so a key in
// the review scene flips it with no per-tile material access / no Rust rebuild):
// 0 = normal height color · 1 = morph-band heatmap (blue fine / green blend / red coarse).
global uniform float wg_dbg_mode;
// Additional mode: wg_dbg_mode=2 uses biome_debug_color, a per-tile page-center WORLD route
// diagnostic. Normal material mode can also use a restrained WORLD-only route tint through
// biome_material_mix; that is a temporary visual-readability bridge, not final per-pixel materials.
// M5 detail: GLOBAL amplitude knob (one RenderingServer.global_shader_parameter for the
// whole scene, set by the harness / gate). 0.0 = no detail (byte-identical to pre-M5).
// Detail is added to VERTEX.y after the displayed base height is formed; the page-data
// contract stays in the facts path. See spec §4 invariant 1.
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
const float WG_DETAIL_FREQ    = 0.00025;  // ~1/4000 m base wavelength. The 5 octaves
                                          // (~4000/2000/1000/500/250 m) stay above the review mesh
                                          // Nyquist after the owner fly mesh was raised to GRID_RES=128.
                                          // (Final freq is an S4 config knob.)
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
	v_page_uv = uv_fine;
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

	float h_lod = mix(h_fine, h_coarse, t);
	// Transient visual fade for newly resident pages: start from parent height, then settle
	// to the normal LOD/morph height. The underlying page data/facts path is unchanged.
	float h = mix(h_coarse, h_lod, clamp(page_fade, 0.0, 1.0));
	v_morph = t;
	// M5 detail: bounded world-space fBm added ON TOP of the base height. Pure function of
	// world.xz -> abutting tiles agree on shared edges (edge-safe). Slice 1 = flat amp
	// (no modulation/fade yet).
	float detail = wg_fbm_detail(world.xz) * wg_detail_amp;
	v_detail_norm = abs(wg_detail_amp) > 1e-6 ? clamp(detail / wg_detail_amp, -1.0, 1.0) : 0.0;
	// v_render_height is the DISPLAYED (debug/height-color) height after the render-side relief
	// multiplier. It is NOT the facts/collision height: collision reads the separate CPU facts path.
	float displayed_h = (h + detail) * relief_scale;
	v_render_height = displayed_h;

	ivec2 page_size = textureSize(height_tex, 0);
	float page_steps = max(float(page_size.x - 1), 1.0);
	vec2 duv = vec2(1.0 / page_steps, 0.0);
	vec2 dvv = vec2(0.0, 1.0 / page_steps);
	float h_l = texture(height_tex, uv_fine - duv).r;
	float h_r = texture(height_tex, uv_fine + duv).r;
	float h_d = texture(height_tex, uv_fine - dvv).r;
	float h_u = texture(height_tex, uv_fine + dvv).r;
	float cell_m = max(world_span / page_steps, 1.0);
	float sx = ((h_r - h_l) * relief_scale) / (2.0 * cell_m);
	float sz = ((h_u - h_d) * relief_scale) / (2.0 * cell_m);
	v_slope = length(vec2(sx, sz));
	v_terrain_normal = normalize(vec3(-sx, 1.0, -sz));
	VERTEX.y = displayed_h;
}

vec3 wg_mountain_palette(float h, float slope) {
	float t = clamp(h / relief_ref * 0.5 + 0.5, 0.0, 1.0);
	vec3 valley = vec3(0.15, 0.31, 0.24);
	vec3 grass = vec3(0.27, 0.43, 0.24);
	vec3 dry = vec3(0.53, 0.47, 0.33);
	vec3 rock = vec3(0.45, 0.44, 0.40);
	vec3 snow = vec3(0.82, 0.84, 0.78);

	vec3 base = valley;
	if (t < 0.35) {
		base = mix(valley, grass, t / 0.35);
	} else if (t < 0.62) {
		base = mix(grass, dry, (t - 0.35) / 0.27);
	} else if (t < 0.82) {
		base = mix(dry, rock, (t - 0.62) / 0.20);
	} else if (t < 0.94) {
		base = mix(rock, snow, (t - 0.82) / 0.12);
	} else {
		base = snow;
	}

	float rock_mix = clamp((slope - 0.22) / 0.28, 0.0, 0.72);
	base = mix(base, rock, rock_mix);
	float cliff_mix = clamp((slope - 0.46) / 0.36, 0.0, 0.45);
	base = mix(base, vec3(0.31, 0.31, 0.29), cliff_mix);
	return base;
}

vec3 wg_apply_terrain_light(vec3 base, vec3 n, float slope) {
	// Manual lighting keeps the shader unshaded for deterministic debug modes while restoring
	// relief readability to the normal view. This changes presentation only, not page data.
	vec3 light_dir = normalize(vec3(-0.45, 0.78, -0.35));
	float ndl = max(dot(normalize(n), light_dir), 0.0);
	float lit = 0.50 + 0.50 * ndl;
	vec3 shaded = base * lit;
	float cool_shadow = clamp(1.0 - ndl, 0.0, 1.0) * 0.16;
	shaded = mix(shaded, vec3(shaded.r * 0.88, shaded.g * 0.94, shaded.b * 1.05), cool_shadow);
	float slope_shadow = clamp((slope - 0.42) / 0.65, 0.0, 0.26);
	return shaded * (1.0 - slope_shadow);
}

vec3 wg_static_reference_material(float code, vec3 base) {
	float cls = floor(code + 0.5);
	if (cls < 0.5) {
		return base;
	}
	if (cls < 1.5) {
		return vec3(0.24, 0.48, 0.35);
	}
	if (cls < 2.5) {
		return vec3(0.44, 0.45, 0.40);
	}
	return vec3(0.74, 0.78, 0.72);
}

void fragment() {
	if (wg_dbg_mode > 1.5) {
		ALBEDO = biome_debug_color.rgb;
	} else if (wg_dbg_mode > 0.5) {
		// morph-band heatmap: blue = pure fine (t=0), red = pure coarse (t=1), green = the blend
		// in between. A hard LOD pop shows as an abrupt blue->red edge with NO green; a proper
		// geomorph shows a WIDE green gradient where fine eases into coarse.
		ALBEDO = vec3(v_morph, 1.0 - abs(v_morph - 0.5) * 2.0, 1.0 - v_morph);
	} else {
		vec3 material = wg_mountain_palette(v_render_height, v_slope) * (1.0 + v_detail_norm * 0.10);
		float material_fade = clamp(page_fade, 0.0, 1.0);
		float static_code = texture(static_material_tex, v_page_uv).r;
		material = mix(material, wg_static_reference_material(static_code, material), clamp(static_material_mix * material_fade, 0.0, 1.0));
		material = mix(material, biome_debug_color.rgb, clamp(biome_material_mix * material_fade, 0.0, 1.0));
		ALBEDO = wg_apply_terrain_light(clamp(material, vec3(0.0), vec3(1.0)), v_terrain_normal, v_slope);
	}
}
