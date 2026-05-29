shader_type spatial;
render_mode unshaded, cull_disabled;

uniform sampler2D height_tex;      // the Texture2DRD from Wg10PageCompute (R32F)
uniform float world_span = 8192.0; // page world span (set from config)
uniform float height_scale = 1.0;  // visual amplitude (config; 1.0 = raw metres)
uniform float relief_ref = 2000.0; // for the color gradient normalization

varying float v_height;

void vertex() {
	// mesh is a flat grid centered at origin spanning [-world_span/2, +world_span/2] in XZ.
	vec2 uv = (VERTEX.xz / world_span) + vec2(0.5);
	float h = texture(height_tex, uv).r;
	v_height = h;
	VERTEX.y = h * height_scale;
}

void fragment() {
	// color by height so relief is visible in the PNG (legible, not pretty).
	float t = clamp(v_height / relief_ref * 0.5 + 0.5, 0.0, 1.0);
	ALBEDO = vec3(t, t * 0.8 + 0.1, 1.0 - t);
}
