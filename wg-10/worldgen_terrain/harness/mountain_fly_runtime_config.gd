extends RefCounted

# Shared renderer/runtime constants for mountain_fly_review.gd and its visual
# capture gate. Producer selection stays in mountain_fly_producers.gd.

const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const NUM_LEVELS := 5
const BASE_SPAN_M := 8192.0
const GRID_RES := 64
const RADIUS_PAGES := 1
const LEAD_SECONDS := 0.5
const MAX_PER_FRAME := 4
const MORPH_REGION_ON := 0.15
const MORPH_REGION_OFF := 0.0
const RELIEF_SCALE := 0.25
const RELIEF_REF := 2000.0
const DETAIL_AMP_M := 350.0
const DEFAULT_MORPH_ENABLED := false
const DEFAULT_DETAIL_ENABLED := false
const SKY := Color(0.45, 0.62, 0.85)

func num_levels() -> int:
	return NUM_LEVELS

func base_span_m() -> float:
	return BASE_SPAN_M

func grid_res() -> int:
	return GRID_RES

func lead_seconds() -> float:
	return LEAD_SECONDS

func detail_amp_m() -> float:
	return DETAIL_AMP_M

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

func configure_streamer(streamer: Object, pool: Object) -> void:
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN_M, RADIUS_PAGES, LEAD_SECONDS, MAX_PER_FRAME)

func configure_rings(rings: Object) -> void:
	rings.call("configure", NUM_LEVELS, BASE_SPAN_M, GRID_RES, SHADER)

func configure_view(view: Object, pool: Object, streamer: Object, rings: Object, morph_enabled: bool) -> void:
	view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN_M, RELIEF_SCALE, morph_region(morph_enabled), RELIEF_REF, LEAD_SECONDS)

func configure_review_environment(env: Environment) -> void:
	var edge := loaded_edge_m()
	env.background_mode = Environment.BG_COLOR
	env.background_color = SKY
	env.fog_enabled = true
	env.fog_mode = Environment.FOG_MODE_DEPTH
	env.fog_depth_begin = edge * 0.45
	env.fog_depth_end = edge * 0.85
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
