extends RefCounted

# Producer/preset configuration for mountain_fly_review.gd.
# This owns the runtime content path choice only; the fly scene still owns
# renderer, input, HUD, and page-stream state.

const PRIM := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE := "res://worldgen_terrain/shaders/biome_page.glsl"
const MOUNTAIN := "res://worldgen_terrain/shaders/biome_mountain.glsl"
const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_page.glsl"
const STATIC_REF_PAYLOAD := "res://worldgen_terrain/generated/review/mountain_network_chunks.json"

const PAGE_PX := 256
const APRON_PX := 160
const CAPACITY := 96
const BASE_SPAN := 8192.0
const WORLD_SEED := 1337
const MOUNTAIN_REVIEW_SEED := 177
const FLOW_ITERS := 192
const FLOW_MAX_LEVEL := 2
# WORLD review stays single-biome-per-page until multi-biome compose moves off the synchronous fly
# stream. Top-2/full compose removes rectangular route pages but currently causes ~1.9s page-build
# hitches in `review_runtime_modes`; treat WORLD as diagnostic, not accepted terrain.
const WORLD_REVIEW_ACTIVE_BIOME_LIMIT := 1
const FEATURE_SPAN_NETWORK_M := 90000.0
const FEATURE_SPAN_CLOSE_DEBUG_M := 3500.0
const RELIEF_M_DEFAULT := 1700.0
const MOUNTAIN_NETWORK_VIEW_RELIEF_SCALE := 0.5
const MOUNTAIN_NETWORK_SOURCE_SCALE := 3.515625
const MOUNTAIN_NETWORK_SOURCE_OFFSET_X_M := 207000.0
const MOUNTAIN_NETWORK_SOURCE_OFFSET_Z_M := 176000.0

const MODE_WORLD := 0
const MODE_MOUNTAIN := 1
const MODE_LEGACY := 2
const MODE_REFERENCE := 3

const PRESET_NETWORK := 0
const PRESET_CLOSE_DEBUG := 1

var _mode := MODE_REFERENCE
var _preset := PRESET_NETWORK
var _relief_m := RELIEF_M_DEFAULT

func configure(pool: Object) -> String:
	if _mode == MODE_WORLD:
		return _configure_world(pool)
	if _mode == MODE_MOUNTAIN:
		return _configure_mountain(pool)
	if _mode == MODE_REFERENCE:
		return _configure_reference(pool)
	return _configure_legacy(pool)

func cycle_mode() -> void:
	if _mode == MODE_MOUNTAIN:
		_mode = MODE_REFERENCE
	elif _mode == MODE_REFERENCE:
		_mode = MODE_LEGACY
	elif _mode == MODE_LEGACY:
		_mode = MODE_WORLD
	else:
		_mode = MODE_MOUNTAIN

func set_mode_label(label: String) -> bool:
	var normalized := label.to_upper()
	if normalized == "WORLD":
		_mode = MODE_WORLD
		return true
	if normalized == "MOUNTAIN":
		_mode = MODE_MOUNTAIN
		return true
	if normalized == "LEGACY":
		_mode = MODE_LEGACY
		return true
	if normalized == "REFERENCE":
		_mode = MODE_REFERENCE
		return true
	return false

func toggle_preset() -> void:
	if _preset == PRESET_NETWORK:
		_preset = PRESET_CLOSE_DEBUG
	else:
		_preset = PRESET_NETWORK

func set_preset_label(label: String) -> bool:
	if label == "network_ref":
		_preset = PRESET_NETWORK
		return true
	if label == "close_debug":
		_preset = PRESET_CLOSE_DEBUG
		return true
	return false

func set_relief_m(value: float) -> void:
	_relief_m = clampf(value, 50.0, 20000.0)

func relief_m() -> float:
	return _relief_m

func runtime_seed() -> int:
	if _mode == MODE_MOUNTAIN or _mode == MODE_REFERENCE:
		return MOUNTAIN_REVIEW_SEED
	return WORLD_SEED

func feature_span_m() -> float:
	if _mode == MODE_REFERENCE:
		return FEATURE_SPAN_NETWORK_M
	if _preset == PRESET_CLOSE_DEBUG:
		return FEATURE_SPAN_CLOSE_DEBUG_M
	return FEATURE_SPAN_NETWORK_M

func source_scale() -> float:
	if _mode == MODE_MOUNTAIN and _preset == PRESET_NETWORK:
		return MOUNTAIN_NETWORK_SOURCE_SCALE
	return 1.0

func source_offset_x_m() -> float:
	if _mode == MODE_MOUNTAIN and _preset == PRESET_NETWORK:
		return MOUNTAIN_NETWORK_SOURCE_OFFSET_X_M
	return 0.0

func source_offset_z_m() -> float:
	if _mode == MODE_MOUNTAIN and _preset == PRESET_NETWORK:
		return MOUNTAIN_NETWORK_SOURCE_OFFSET_Z_M
	return 0.0

func preset_label() -> String:
	if _preset == PRESET_CLOSE_DEBUG:
		return "close_debug"
	return "network_ref"

func mode_label() -> String:
	if _mode == MODE_WORLD:
		return "WORLD"
	if _mode == MODE_MOUNTAIN:
		return "MOUNTAIN"
	if _mode == MODE_REFERENCE:
		return "REFERENCE"
	return "LEGACY"

func is_world() -> bool:
	return _mode == MODE_WORLD

func is_legacy() -> bool:
	return _mode == MODE_LEGACY

func is_reference() -> bool:
	return _mode == MODE_REFERENCE

func world_active_biome_limit() -> int:
	return WORLD_REVIEW_ACTIVE_BIOME_LIMIT

func view_relief_scale(default_scale: float) -> float:
	if _mode == MODE_REFERENCE:
		return 1.0
	if _mode == MODE_MOUNTAIN and _preset == PRESET_NETWORK:
		return MOUNTAIN_NETWORK_VIEW_RELIEF_SCALE
	return default_scale

func view_relief_ref(default_ref: float, default_scale: float) -> float:
	if _mode == MODE_LEGACY:
		return default_ref
	return maxf(50.0, _relief_m * view_relief_scale(default_scale))

func _configure_world(pool: Object) -> String:
	var err := str(pool.call("configure_biome_world",
		ProjectSettings.globalize_path(PACK_RES_DIR),
		PACK_FILE,
		CAPACITY, PAGE_PX, APRON_PX, BASE_SPAN, feature_span_m(), FLOW_ITERS, _relief_m, FLOW_MAX_LEVEL, runtime_seed()))
	if err != "":
		return err
	return str(pool.call("set_biome_world_active_limit", WORLD_REVIEW_ACTIVE_BIOME_LIMIT))

func _configure_mountain(pool: Object) -> String:
	var err := str(pool.call("configure_biome",
		ProjectSettings.globalize_path(PRIM),
		ProjectSettings.globalize_path(MACHINE),
		ProjectSettings.globalize_path(MOUNTAIN),
		CAPACITY, PAGE_PX, APRON_PX, BASE_SPAN, feature_span_m(), FLOW_ITERS, _relief_m, FLOW_MAX_LEVEL, runtime_seed()))
	if err != "":
		return err
	return str(pool.call("set_biome_source_transform", source_scale(), source_offset_x_m(), source_offset_z_m()))

func _configure_reference(pool: Object) -> String:
	return str(pool.call("configure_static_reference",
		ProjectSettings.globalize_path(STATIC_REF_PAYLOAD),
		CAPACITY, PAGE_PX, BASE_SPAN, runtime_seed()))

func _configure_legacy(pool: Object) -> String:
	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	return str(pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, runtime_seed()))
