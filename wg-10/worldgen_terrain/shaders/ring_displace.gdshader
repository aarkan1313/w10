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

varying float v_height;
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

	float h = mix(h_fine, h_coarse, t);
	v_height = h;
	v_morph = t;
	VERTEX.y = h * height_scale;
}

void fragment() {
	if (wg_dbg_mode > 0.5) {
		// morph-band heatmap: blue = pure fine (t=0), red = pure coarse (t=1), green = the blend
		// in between. A hard LOD pop shows as an abrupt blue->red edge with NO green; a proper
		// geomorph shows a WIDE green gradient where fine eases into coarse.
		ALBEDO = vec3(v_morph, 1.0 - abs(v_morph - 0.5) * 2.0, 1.0 - v_morph);
	} else {
		float t = clamp(v_height / relief_ref * 0.5 + 0.5, 0.0, 1.0);
		ALBEDO = vec3(t, t * 0.8 + 0.1, 1.0 - t);
	}
}
