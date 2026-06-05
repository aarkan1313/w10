extends RefCounted

# Per-rung producer configuration for the un-intercept proving ladder.
# Each rung selects ONE Wg10PagePool producer path + scale/seed/source-window/flow.
# The ladder flips one baked crutch -> live procedural per rung and gates convergence
# toward the accepted baked REFERENCE. Reference constants are lifted from
# mountain_fly_producers.gd to match the accepted baseline exactly.

const PRIM := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE := "res://worldgen_terrain/shaders/biome_page.glsl"
const MOUNTAIN := "res://worldgen_terrain/shaders/biome_mountain.glsl"
const ACCEPTED_WORLD_LAYER_PAYLOAD := "res://worldgen_terrain/generated/review/mountain_world_layer_tiles.json"

const PAGE_PX := 256
const APRON_PX := 160
const CAPACITY := 96
const BASE_SPAN := 8192.0
const MOUNTAIN_REVIEW_SEED := 177
# Flow ON/OFF is gated by flow_max_level (a level-L page runs flow iff L < flow_max_level), NOT by
# flow_iters. flow_iters MUST stay >= 1: the scheduler's discharge-buffer parity assert
# (scheduler.rs:243) panics at flow_iters=0. So: macro/off = flow_max_level 0 (level 0 not < 0 -> no
# flow); flow/on = flow_max_level 2 (level 0 < 2 -> flow). flow_iters stays 192 for both.
const FLOW_ITERS := 192
const FLOW_MAX_LEVEL_OFF := 0
const FLOW_MAX_LEVEL_ON := 2
const FEATURE_SPAN_NETWORK_M := 90000.0
const RELIEF_M_DEFAULT := 1700.0
# Accepted source-window transform — VERIFIED exact (mountain_fly_review_smoke_check.gd:361-363).
# Applied as DIRECT offsets: source = display * scale + offset.
const SOURCE_SCALE := 3.515625
const SOURCE_OFFSET_X_M := 207000.0
const SOURCE_OFFSET_Z_M := 176000.0

# Rung 0 analytic plumbing (known closed-form height).
const ANALYTIC_AMP := 300.0
const ANALYTIC_LAMBDA := 4000.0

const RUNG_REFERENCE := "reference"
const RUNG_ANALYTIC := "analytic"
const RUNG_MOUNTAIN_MACRO := "mountain_macro"  # live recipe, flow OFF, reference scale
const RUNG_MOUNTAIN_FLOW := "mountain_flow"    # live recipe, flow ON, reference scale

const _KNOWN_RUNGS := [
	RUNG_REFERENCE,
	RUNG_ANALYTIC,
	RUNG_MOUNTAIN_MACRO,
	RUNG_MOUNTAIN_FLOW,
]

var _rung := RUNG_REFERENCE

func set_rung(rung: String) -> bool:
	if rung in _KNOWN_RUNGS:
		_rung = rung
		return true
	return false

func rung() -> String:
	return _rung

func relief_m() -> float:
	# Analytic rung renders ~ANALYTIC_AMP relief; others use the accepted mountain relief.
	if _rung == RUNG_ANALYTIC:
		return ANALYTIC_AMP
	return RELIEF_M_DEFAULT

func configure(pool: Object) -> String:
	match _rung:
		RUNG_REFERENCE:
			return _configure_reference(pool)
		RUNG_ANALYTIC:
			return _configure_analytic(pool)
		RUNG_MOUNTAIN_MACRO:
			return _configure_live_mountain(pool, FLOW_MAX_LEVEL_OFF)
		RUNG_MOUNTAIN_FLOW:
			return _configure_live_mountain(pool, FLOW_MAX_LEVEL_ON)
		_:
			return "ladder_producers: unknown rung %s" % _rung

func _configure_reference(pool: Object) -> String:
	return str(pool.call("configure_static_reference",
		ProjectSettings.globalize_path(ACCEPTED_WORLD_LAYER_PAYLOAD),
		CAPACITY, PAGE_PX, BASE_SPAN, MOUNTAIN_REVIEW_SEED))

func _configure_analytic(pool: Object) -> String:
	return str(pool.call("configure_analytic",
		CAPACITY, PAGE_PX, BASE_SPAN, ANALYTIC_AMP, ANALYTIC_LAMBDA))

# Live mountain recipe at the accepted scale/seed/source-window. NO reference binding, so
# dispatch_page_compute reaches compute_biome_page_cached. flow_max_level selects flow on/off for the
# level-0 page (flow_iters stays >=1 — flow_iters=0 panics the scheduler discharge assert).
func _configure_live_mountain(pool: Object, flow_max_level: int) -> String:
	var err := str(pool.call("configure_biome",
		ProjectSettings.globalize_path(PRIM),
		ProjectSettings.globalize_path(MACHINE),
		ProjectSettings.globalize_path(MOUNTAIN),
		CAPACITY, PAGE_PX, APRON_PX, BASE_SPAN, FEATURE_SPAN_NETWORK_M, FLOW_ITERS, RELIEF_M_DEFAULT, flow_max_level, MOUNTAIN_REVIEW_SEED))
	if err != "":
		return err
	# set_biome_source_transform requires SingleBiome/World already configured (so call it AFTER).
	return str(pool.call("set_biome_source_transform", SOURCE_SCALE, SOURCE_OFFSET_X_M, SOURCE_OFFSET_Z_M))
