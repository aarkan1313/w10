shader_type spatial;
render_mode unshaded, cull_disabled;

uniform sampler2D height_tex;            // this level's R32F page (pool Texture2DRD)
uniform sampler2D coarse_height_tex;     // next-coarser level's page (for the morph)
uniform float world_span = 8192.0;       // this level's band world span
uniform float coarse_span = 8192.0;      // the coarser level's band world span
uniform vec2 coarse_origin = vec2(0.0);  // the coarser page's WORLD corner (lower-XZ); page covers [origin, origin+coarse_span]
uniform float height_scale = 1.0;        // visual amplitude (config; 1.0 = raw metres)
uniform float morph_region = 0.0;        // transition width as a fraction of span (0 = no morph)
uniform float relief_ref = 2000.0;       // color gradient normalization

varying float v_height;

// World XZ of this vertex = mesh-local XZ + the instance origin (rings recenter by
// translating the instance). MODEL_MATRIX maps local -> world.
void vertex() {
	vec3 world = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;

	// This level samples its own page by local UV: local XZ in [-span/2, span/2] -> [0,1].
	vec2 uv_fine = (VERTEX.xz / world_span) + vec2(0.5);
	float h_fine = texture(height_tex, uv_fine).r;

	// Morph factor: 0 in the interior, rising to 1 at the outer edge over the transition
	// region. Use the Chebyshev (max-of-axes) distance from the level center, so the
	// transition is a square band matching the square ring edge. cheb in [0,1] where 1
	// is the outer edge.
	float half_span = world_span * 0.5;
	float cheb = max(abs(VERTEX.x), abs(VERTEX.z)) / half_span; // 0 center .. 1 outer edge
	float region = max(morph_region, 1e-6);
	float t = clamp((cheb - (1.0 - region)) / region, 0.0, 1.0); // 0 until inner edge of region

	// Coarser page covers world [coarse_origin, coarse_origin + coarse_span]; sample it
	// corner-relative so the morph is correct wherever the coarse page sits (Fix #2).
	vec2 uv_coarse = (world.xz - coarse_origin) / coarse_span;
	float h_coarse = texture(coarse_height_tex, uv_coarse).r;

	float h = mix(h_fine, h_coarse, t);
	v_height = h;
	VERTEX.y = h * height_scale;
}

void fragment() {
	float t = clamp(v_height / relief_ref * 0.5 + 0.5, 0.0, 1.0);
	ALBEDO = vec3(t, t * 0.8 + 0.1, 1.0 - t);
}
