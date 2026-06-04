//! Headless state-machine coverage for page pool lifecycle fixes.

use super::*;
use crate::pack::{Pack, GrammarConstants};
use crate::gpu_compute::PackBuffers;
use crate::page_compute::PageComputeContext;
use crate::page_policy::PagePolicy;
use std::collections::BTreeMap;

fn minimal_pack() -> Pack {
    Pack {
        grammar_constants: GrammarConstants {
            region_size_m: 500.0,
            province_size_regions: 8,
            palette_primary_pct: 60,
            palette_compatible_pct: 30,
            moderation_min: 0.4,
            moderation_strength: 0.5,
        },
        palettes: Vec::new(),
        compatibility: BTreeMap::new(),
        family_ids: Vec::new(),
        family_kernels: BTreeMap::new(),
    }
}

fn minimal_pack_buffers() -> PackBuffers {
    PackBuffers {
        palettes_bytes:    Vec::new(),
        compat_off_bytes:  Vec::new(),
        compat_flat_bytes: Vec::new(),
        krec_bytes:        Vec::new(),
        kparam_bytes:      Vec::new(),
        kdata_bytes:       Vec::new(),
        num_palettes:      0,
    }
}

// A PageComputeContext is all POD `Rid` handles. `Rid::new(..)` does NOT touch
// the GPU — it just wraps a u64 — so this is safe to build/drop headlessly. We
// only need it to be `Some(..)` so the reset can prove it goes to `None`.
fn fake_compute_ctx() -> PageComputeContext {
    PageComputeContext {
        shader:      Rid::new(101),
        pipeline:    Rid::new(102),
        palettes:    Rid::new(103),
        compat_off:  Rid::new(104),
        compat_flat: Rid::new(105),
        krec:        Rid::new(106),
        kparam:      Rid::new(107),
        kdata:       Rid::new(108),
    }
}

/// F7 — the core invariant. A FULLY-configured field set (policy/pack/
/// pack_buffers/glsl_source/compute_ctx all Some, slot_tex populated) must,
/// after `reset_configured_state`, leave EVERY field None/empty. The bug was
/// that the old `free_all_impl` cleared only `compute_ctx` + the slot vectors,
/// leaving the four config Options Some — so the acquire guard PASSED while
/// `compute_ctx` was None, then `compute_ctx.as_ref().unwrap()` panicked.
#[test]
fn reset_clears_all_configured_state_no_half_configured_residue() {
    let mut policy:       Option<PagePolicy>          = Some(PagePolicy::new(4));
    // Populated with POD RIDs (the GPU free happens BEFORE reset in the real
    // code path; here we only assert the handles are dropped to None).
    let mut slot_tex:     Vec<Option<Rid>>            =
        vec![Some(Rid::new(1)), None, Some(Rid::new(2)), None];
    // slot_wrap holds Gd<Texture2Drd> which can't be built headlessly; the
    // realistic populated case is covered by slot_tex. Use the post-config
    // shape: a sized Vec of None (what `configure` builds before any acquire).
    let mut slot_wrap:    Vec<Option<Gd<Texture2Drd>>> = (0..4).map(|_| None).collect();
    let mut slot_material_tex: Vec<Option<Rid>> =
        vec![Some(Rid::new(11)), None, Some(Rid::new(12)), None];
    let mut slot_material_wrap: Vec<Option<Gd<Texture2Drd>>> = (0..4).map(|_| None).collect();
    let mut pack:         Option<Pack>                = Some(minimal_pack());
    let mut pack_buffers: Option<PackBuffers>         = Some(minimal_pack_buffers());
    let mut glsl_source:  Option<String>              = Some("// glsl".to_string());
    let mut compute_ctx:  Option<PageComputeContext>  = Some(fake_compute_ctx());
    // Biome path fields: a BiomePageComputeContext has private fields (a real ApronBuffers set)
    // so it can't be faked headlessly; the legacy-path reset of these two is covered by passing
    // a default flag/None here (the biome reset to None is trivial + asserted below).
    let mut use_biome_path: bool = false;
    let mut biome_ctx: Option<biome_page_compute::BiomePageComputeContext> = None;
    let mut biome_world: Option<BiomeWorldRuntime> = None;
    let mut static_ref: Option<StaticHeightRuntime> = None;

    Wg10PagePool::reset_configured_state(
        &mut policy,
        &mut slot_tex,
        &mut slot_wrap,
        &mut slot_material_tex,
        &mut slot_material_wrap,
        &mut pack,
        &mut pack_buffers,
        &mut glsl_source,
        &mut compute_ctx,
        &mut use_biome_path,
        &mut biome_ctx,
        &mut biome_world,
        &mut static_ref,
    );

    // Every configure-set field is None — the acquire guard now correctly sees
    // "not configured" (returns None) instead of passing then unwrapping None.
    assert!(policy.is_none(),       "policy must be cleared");
    assert!(pack.is_none(),         "pack must be cleared");
    assert!(pack_buffers.is_none(), "pack_buffers must be cleared");
    assert!(glsl_source.is_none(),  "glsl_source must be cleared");
    assert!(compute_ctx.is_none(),  "compute_ctx must be cleared");
    assert!(!use_biome_path,        "use_biome_path must be cleared");
    assert!(biome_ctx.is_none(),    "biome_ctx must be cleared");
    assert!(biome_world.is_none(),  "biome_world must be cleared");
    assert!(static_ref.is_none(),   "static_ref must be cleared");
    // Slot vectors emptied — no stale slot_wrap indexable by a stale policy.
    assert!(slot_tex.is_empty(),    "slot_tex must be empty");
    assert!(slot_wrap.is_empty(),   "slot_wrap must be empty");
    assert!(slot_material_tex.is_empty(), "slot_material_tex must be empty");
    assert!(slot_material_wrap.is_empty(), "slot_material_wrap must be empty");

    // THE F7 GUARD CHECK: the acquire-guard predicate (policy && pack &&
    // pack_buffers && glsl_source all Some) and `compute_ctx.is_some()` must
    // AGREE. The old bug made the guard true while compute_ctx was None — that
    // disagreement is the panic. Assert they cannot disagree after a reset.
    let guard_passes = policy.is_some()
        && pack.is_some()
        && pack_buffers.is_some()
        && glsl_source.is_some();
    assert!(!guard_passes, "acquire guard must FAIL after reset (unconfigured)");
    assert_eq!(
        guard_passes,
        compute_ctx.is_some(),
        "F7: acquire guard and compute_ctx presence must never disagree"
    );
}

/// F7/F8 — `reset_configured_state` is idempotent: calling it on an already-
/// unconfigured (fresh) field set is a harmless no-op. This is what makes
/// `free_all()` safe to call twice and `configure`'s free-before-reconfigure
/// safe on a never-configured pool.
#[test]
fn reset_is_idempotent_on_unconfigured_state() {
    let mut policy:       Option<PagePolicy>           = None;
    let mut slot_tex:     Vec<Option<Rid>>             = Vec::new();
    let mut slot_wrap:    Vec<Option<Gd<Texture2Drd>>> = Vec::new();
    let mut slot_material_tex: Vec<Option<Rid>> = Vec::new();
    let mut slot_material_wrap: Vec<Option<Gd<Texture2Drd>>> = Vec::new();
    let mut pack:         Option<Pack>                 = None;
    let mut pack_buffers: Option<PackBuffers>          = None;
    let mut glsl_source:  Option<String>               = None;
    let mut compute_ctx:  Option<PageComputeContext>   = None;
    let mut use_biome_path: bool                       = false;
    let mut biome_ctx: Option<biome_page_compute::BiomePageComputeContext> = None;
    let mut biome_world: Option<BiomeWorldRuntime> = None;
    let mut static_ref: Option<StaticHeightRuntime> = None;

    // Must not panic / must stay fully unconfigured.
    Wg10PagePool::reset_configured_state(
        &mut policy, &mut slot_tex, &mut slot_wrap,
        &mut slot_material_tex, &mut slot_material_wrap,
        &mut pack, &mut pack_buffers, &mut glsl_source, &mut compute_ctx,
        &mut use_biome_path, &mut biome_ctx, &mut biome_world, &mut static_ref,
    );

    assert!(policy.is_none() && pack.is_none() && pack_buffers.is_none()
        && glsl_source.is_none() && compute_ctx.is_none());
    assert!(
        !use_biome_path
            && biome_ctx.is_none()
            && biome_world.is_none()
            && static_ref.is_none()
    );
    assert!(
        slot_tex.is_empty()
            && slot_wrap.is_empty()
            && slot_material_tex.is_empty()
            && slot_material_wrap.is_empty()
    );
}

/// `is_configured` mirrors the acquire guard exactly: true ONLY when all four
/// of policy/pack/pack_buffers/glsl_source are Some. This is the predicate
/// `configure` uses to decide whether to free-before-reconfigure (F8), so it
/// must match the guard wording in `acquire_page`. Verified field-by-field via
/// the same boolean the guard computes (the struct itself needs a Base, so we
/// reproduce the predicate here rather than instantiate the GodotClass).
#[test]
fn configured_predicate_requires_all_four_config_options() {
    // Helper mirroring the guard / `is_configured` over the four options.
    fn guard(p: bool, pk: bool, pb: bool, g: bool) -> bool { p && pk && pb && g }

    assert!(guard(true, true, true, true), "all Some => configured");
    // Any single missing option => NOT configured (guard returns early).
    assert!(!guard(false, true, true, true), "missing policy => unconfigured");
    assert!(!guard(true, false, true, true), "missing pack => unconfigured");
    assert!(!guard(true, true, false, true), "missing pack_buffers => unconfigured");
    assert!(!guard(true, true, true, false), "missing glsl_source => unconfigured");
    assert!(!guard(false, false, false, false), "all None => unconfigured");
}

/// A FRESH pool is configured on NEITHER path. `Wg10PagePool` is a GodotClass needing a live
/// `Base<RefCounted>`, so it cannot be constructed under `cargo test` (like every other test in
/// this module); we therefore mirror the new biome-aware `is_configured` predicate over plain
/// booleans — the exact shape a fresh `init()` produces: policy None, both legacy + biome ctx
/// absent. The end-to-end GodotClass path is the windowed m3 gate (Task 7). The point this pins:
/// adding the biome OR-branch must NOT make an UNconfigured pool report configured.
#[test]
fn fresh_pool_not_configured_either_path() {
    // Mirror is_configured: policy && (legacy || single-biome || world-biome || static-ref).
    fn is_configured(
        policy: bool, pack: bool, pack_buffers: bool, glsl: bool, compute_ctx: bool,
        biome_ctx: bool,
        biome_world: bool,
        static_ref: bool,
    ) -> bool {
        policy
            && ((pack && pack_buffers && glsl && compute_ctx)
                || biome_ctx
                || biome_world
                || static_ref)
    }

    // Fresh pool (what `init()` sets): everything absent -> NOT configured on either path.
    assert!(
        !is_configured(false, false, false, false, false, false, false, false),
        "fresh pool must be unconfigured on both paths"
    );
    // Policy alone (no producer ctx on EITHER path) is still unconfigured.
    assert!(!is_configured(true, false, false, false, false, false, false, false), "policy alone => unconfigured");
    // Legacy path fully built => configured.
    assert!(is_configured(true, true, true, true, true, false, false, false), "full legacy => configured");
    // Legacy path missing its compute_ctx => unconfigured (the F7 hazard, now guarded).
    assert!(!is_configured(true, true, true, true, false, false, false, false), "legacy w/o compute_ctx => unconfigured");
    // Biome path => configured with ONLY policy + biome_ctx (no pack/glsl).
    assert!(is_configured(true, false, false, false, false, true, false, false), "biome path => configured");
    // Biome ctx present but NO policy => unconfigured (policy is mandatory on both paths).
    assert!(!is_configured(false, false, false, false, false, true, false, false), "biome ctx w/o policy => unconfigured");
    assert!(is_configured(true, false, false, false, false, false, true, false), "biome world => configured");
    assert!(!is_configured(false, false, false, false, false, false, true, false), "biome world w/o policy => unconfigured");
    assert!(is_configured(true, false, false, false, false, false, false, true), "static reference => configured");
    assert!(!is_configured(false, false, false, false, false, false, false, true), "static reference w/o policy => unconfigured");
}

// NOTE (windowed-only, NOT run headless): the END-TO-END proofs —
//   (1) `free_all()` then `acquire_page(..)` returns None without panicking, and
//   (2) a second `configure(..)` frees the prior textures' RIDs + compute ctx
//       (no GPU leak) and leaves no duplicate slot vectors —
// require a live windowed RenderingDevice (the pool's `configure`/`acquire_page`/
// `free_all_impl` all call `RenderingServer::singleton().get_rendering_device()`
// and dispatch real GPU work) and a constructed `Gd<Wg10PagePool>` (needs the
// engine). Run the windowed gpu/m3 gate (editor closed) to confirm those paths.
// The state-machine fix itself is covered by the headless tests above.
