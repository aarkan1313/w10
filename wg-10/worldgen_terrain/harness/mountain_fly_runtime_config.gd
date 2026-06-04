extends RefCounted

# Shared renderer/runtime constants for mountain_fly_review.gd and its visual
# capture gate. Producer selection stays in mountain_fly_producers.gd.

const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const NUM_LEVELS := 5
const BASE_SPAN_M := 8192.0
# The accepted mountain-network payload is sampled at about 66.7 m. A 128x128 fly mesh samples
# the 8192 m base page at 64 m, which preserves data but still reads as triangular facets in the
# owner fly. 256 oversamples the accepted height texture for smoother presentation without changing
# the page/facts data contract.
const GRID_RES := 256
const RADIUS_PAGES := 1
const LEAD_SECONDS := 0.5
const MAX_PER_FRAME := 1
const MORPH_REGION_ON := 0.15
const MORPH_REGION_OFF := 0.0
const RELIEF_SCALE := 0.25
# Default color-gradient reference. Producer modes override this from their displayed relief so
# low-relief WORLD/MOUNTAIN views do not collapse into one washed-out palette.
const RELIEF_REF := 1700.0
const DETAIL_AMP_M := 350.0
const DEFAULT_MORPH_ENABLED := false
# Owner review opens on the accepted mountain-network baseline. Keep procedural
# display detail opt-in through N so modes 1/2/3 are not all contaminated by the
# same synthetic close-surface noise.
const DEFAULT_DETAIL_ENABLED := false
const SKY := Color(0.45, 0.62, 0.85)
# Accepted mountain-network display footprint. The streamer keeps a larger loaded edge for
# fallback coverage, but the owner review camera/fog should not expose static-reference samples
# outside the accepted 9x9 payload.
const REVIEW_VISUAL_EDGE_M := 76800.0

func num_levels() -> int:
	return NUM_LEVELS

func base_span_m() -> float:
	return BASE_SPAN_M

func grid_res() -> int:
	return GRID_RES

func lead_seconds() -> float:
	return LEAD_SECONDS

func max_per_frame() -> int:
	return MAX_PER_FRAME

func detail_amp_m() -> float:
	return DETAIL_AMP_M

func default_relief_scale() -> float:
	return RELIEF_SCALE

func default_relief_ref() -> float:
	return RELIEF_REF

func default_morph_enabled() -> bool:
	return DEFAULT_MORPH_ENABLED

func default_detail_enabled() -> bool:
	return DEFAULT_DETAIL_ENABLED

func sky_color() -> Color:
	return SKY

func morph_region(enabled: bool) -> float:
	return MORPH_REGION_ON if enabled else MORPH_REGION_OFF

func loaded_edge_m() -> float:
	var coarsest_span := BASE_SPAN_M * pow(2.0, NUM_LEVELS - 1)
	return (RADIUS_PAGES + 0.5) * coarsest_span

func review_visual_edge_m() -> float:
	return REVIEW_VISUAL_EDGE_M

func configure_streamer(streamer: Object, pool: Object) -> void:
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN_M, RADIUS_PAGES, LEAD_SECONDS, MAX_PER_FRAME)

func configure_rings(rings: Object) -> void:
	rings.call("configure", NUM_LEVELS, BASE_SPAN_M, GRID_RES, SHADER)

func configure_view(view: Object, pool: Object, streamer: Object, rings: Object, morph_enabled: bool, relief_scale := RELIEF_SCALE, relief_ref := RELIEF_REF) -> void:
	view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN_M, relief_scale, morph_region(morph_enabled), relief_ref, LEAD_SECONDS)

func configure_review_environment(env: Environment) -> void:
	var edge := REVIEW_VISUAL_EDGE_M
	env.background_mode = Environment.BG_COLOR
	env.background_color = SKY
	env.fog_enabled = true
	env.fog_mode = Environment.FOG_MODE_DEPTH
	env.fog_depth_begin = edge * 0.38
	env.fog_depth_end = edge * 0.58
	env.fog_light_color = SKY

func register_shader_globals(detail_enabled: bool) -> void:
	RenderingServer.global_shader_parameter_add("wg_dbg_mode", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, 0.0)
	RenderingServer.global_shader_parameter_add("wg_detail_amp", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, 0.0)
	set_debug_mode(0)
	set_detail_enabled(detail_enabled)

func set_debug_mode(mode: int) -> void:
	RenderingServer.global_shader_parameter_set("wg_dbg_mode", float(mode))

func set_detail_enabled(enabled: bool) -> void:
	RenderingServer.global_shader_parameter_set("wg_detail_amp", DETAIL_AMP_M if enabled else 0.0)
