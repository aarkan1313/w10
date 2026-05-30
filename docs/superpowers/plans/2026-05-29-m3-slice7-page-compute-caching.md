# WorldGen10 M3 Slice 7 — Page-Compute Resource Caching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cache the per-page-invariant GPU compute resources (compiled shader, pipeline, the six pack buffers) once in `Wg10PagePool`, so page production stops recompiling the shader + re-uploading the ~25 MB atlas every page — eliminating the 90 ms boundary-crossing spike and turning the p99 acceptance gate green.

**Architecture:** A `PageComputeContext` (8 cached RIDs) is built once at `Wg10PagePool::configure` and freed at `free_all` (the pool stays the single RID owner). A new `compute_page_cached` does the per-page work — build a uniform set (cached buffers + this page's target image), set the push constant, dispatch (fire-and-forget) — reusing the cached shader/pipeline/buffers. The old per-page `compute_into_texture` (which rebuilt everything) is replaced. The `m3_accept_check` gate gains a `compute_ms_max` ceiling; p99 goes green. Zero scheduler/view/rings/shader change.

**Tech Stack:** Rust (gdext 0.5.3, godot api-4-6, RenderingDevice global device), windowed gate via `tools/gate.py`.

---

## Conventions (read before Task 1)

- **Build/test** from `wg-10/rust`, `CARGO_TARGET_DIR` UNSET: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo build` / `... cargo test`.
- **Windowed gate** — the CONTROLLER runs it (`python tools/gate.py --suite m3`, GODOT_BIN set) + reads the printed p99/compute_ms_max + validates sane.
- **Commit trailer:** `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`. Stay on `main`.
- The current `compute_into_texture` (in `page_compute.rs`, ~lines 105–215) does, PER CALL:
  strip GLSL → `shader_compile_spirv_from_source` → `shader_create_from_spirv` → create 6
  `storage_buffer_create` from `PackBuffers` bytes → build uniform set (image binding 0 + buffers
  3–8) → push constant → `compute_list_*` dispatch (no submit/sync — global RD auto-submits) →
  free the 6 buffers + pipeline + shader. The shader/pipeline/buffers are identical every call;
  only origin/span/page_px (push constant) and `target_rid` (binding 0) vary.
- Reused helpers (keep): `build_page_push_constant`, `make_image_uniform`, `make_storage_uniform`,
  `bytes_to_pba`, `build_pack_buffers`/`PackBuffers`.

---

## File Structure

**Modify:** `wg-10/rust/src/page_compute.rs` — add `PageComputeContext` + `build_page_compute_context` + `free_page_compute_context` + `compute_page_cached`; remove the old per-page `compute_into_texture`.
**Modify:** `wg-10/rust/src/page_pool.rs` — store `Option<PageComputeContext>`; build at `configure`, use `compute_page_cached` in both acquire arms, free at `free_all`.
**Modify:** `wg-10/worldgen_terrain/tests/m3_accept_check.gd` — add the `compute_ms_max` ceiling assertion.

---

## Task 1: `PageComputeContext` + build/free/dispatch in `page_compute.rs`

**Files:** Modify `wg-10/rust/src/page_compute.rs`

Hoist the per-page-invariant work (shader compile, pipeline, 6 buffers) into a cached context;
add a per-page dispatch that reuses it.

- [ ] **Step 1: Add the context struct + builder + free + cached dispatch**

In `wg-10/rust/src/page_compute.rs`, add (the builder/free mirror the existing
`compute_into_texture` build/cleanup logic, just hoisted to run once):

```rust
/// Per-page-INVARIANT GPU compute resources, built once and reused for every page. Owned by
/// Wg10PagePool (built at configure, freed at free_all). Only the push constant (origin/span)
/// and the target image (binding 0) vary per page; everything here is identical for all pages.
pub(crate) struct PageComputeContext {
    pub shader: Rid,
    pub pipeline: Rid,
    pub palettes: Rid,
    pub compat_off: Rid,
    pub compat_flat: Rid,
    pub krec: Rid,
    pub kparam: Rid,
    pub kdata: Rid,
}

/// Build the cached compute context ONCE: compile the shader, create the pipeline, upload the
/// six pack buffers. Returns Err with a descriptive message on compile/create failure (the pool
/// surfaces it from configure). The ~25 MB kernel atlas is uploaded here ONCE, not per page.
pub(crate) fn build_page_compute_context(
    rd: &mut Gd<RenderingDevice>,
    pb: &PackBuffers,
    glsl_source: &str,
) -> Result<PageComputeContext, String> {
    // strip Godot annotations + compile SPIRV
    let glsl_stripped: String = glsl_source.lines()
        .filter(|l| !l.trim_start().starts_with("#["))
        .collect::<Vec<_>>()
        .join("\n");
    let mut src = RdShaderSource::new_gd();
    src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);
    let spirv = rd.shader_compile_spirv_from_source(&src)
        .ok_or_else(|| "build_page_compute_context: shader_compile_spirv_from_source returned null".to_string())?;
    {
        let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
        if !err.is_empty() {
            return Err(format!("build_page_compute_context: GLSL compile error: {err}"));
        }
    }
    let shader = rd.shader_create_from_spirv(&spirv);
    if shader.is_invalid() {
        return Err("build_page_compute_context: shader_create_from_spirv returned invalid RID".to_string());
    }
    let pipeline = rd.compute_pipeline_create(shader);
    if pipeline.is_invalid() {
        rd.free_rid(shader);
        return Err("build_page_compute_context: compute_pipeline_create returned invalid RID".to_string());
    }

    // upload the six pack buffers ONCE
    let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size exceeds u32") };
    let palettes    = rd.storage_buffer_create_ex(bsize(pb.palettes_bytes.len())).data(&bytes_to_pba(&pb.palettes_bytes)).done();
    let compat_off  = rd.storage_buffer_create_ex(bsize(pb.compat_off_bytes.len())).data(&bytes_to_pba(&pb.compat_off_bytes)).done();
    let compat_flat = rd.storage_buffer_create_ex(bsize(pb.compat_flat_bytes.len())).data(&bytes_to_pba(&pb.compat_flat_bytes)).done();
    let krec        = rd.storage_buffer_create_ex(bsize(pb.krec_bytes.len())).data(&bytes_to_pba(&pb.krec_bytes)).done();
    let kparam      = rd.storage_buffer_create_ex(bsize(pb.kparam_bytes.len())).data(&bytes_to_pba(&pb.kparam_bytes)).done();
    let kdata       = rd.storage_buffer_create_ex(bsize(pb.kdata_bytes.len())).data(&bytes_to_pba(&pb.kdata_bytes)).done();

    Ok(PageComputeContext { shader, pipeline, palettes, compat_off, compat_flat, krec, kparam, kdata })
}

/// Free all cached compute RIDs. Call from the pool's free_all. The shader free cascades any
/// uniform sets created against it; per-page uniform sets are freed per page in compute_page_cached.
pub(crate) fn free_page_compute_context(rd: &mut Gd<RenderingDevice>, ctx: &PageComputeContext) {
    rd.free_rid(ctx.palettes);
    rd.free_rid(ctx.compat_off);
    rd.free_rid(ctx.compat_flat);
    rd.free_rid(ctx.krec);
    rd.free_rid(ctx.kparam);
    rd.free_rid(ctx.kdata);
    rd.free_rid(ctx.pipeline);
    rd.free_rid(ctx.shader);
}

/// Dispatch one page into `target_rid` using the CACHED context. Per-page work only: build the
/// uniform set (cached buffers + this page's image), push constant, dispatch (fire-and-forget on
/// the global RD), free the per-page uniform set. No recompile, no buffer re-upload.
pub(crate) fn compute_page_cached(
    rd: &mut Gd<RenderingDevice>,
    ctx: &PageComputeContext,
    gc: &pack::GrammarConstants,
    num_palettes: i32,
    target_rid: Rid,
    origin_x: f64,
    origin_z: f64,
    world_span: f64,
    page_px: i64,
    seed: i64,
) -> Result<(), String> {
    let mut uniforms: Array<Gd<RdUniform>> = Array::new();
    uniforms.push(&make_image_uniform(0, target_rid));
    uniforms.push(&make_storage_uniform(3, ctx.palettes));
    uniforms.push(&make_storage_uniform(4, ctx.compat_off));
    uniforms.push(&make_storage_uniform(5, ctx.compat_flat));
    uniforms.push(&make_storage_uniform(6, ctx.krec));
    uniforms.push(&make_storage_uniform(7, ctx.kparam));
    uniforms.push(&make_storage_uniform(8, ctx.kdata));
    let uset = rd.uniform_set_create(&uniforms, ctx.shader, 0);
    if uset.is_invalid() {
        return Err("compute_page_cached: uniform_set_create returned invalid RID".to_string());
    }

    let push_bytes = build_page_push_constant(
        gc, seed as i32, num_palettes,
        origin_x as f32, origin_z as f32, world_span as f32, page_px as i32,
    );
    let push_pba = bytes_to_pba(&push_bytes);

    let px = page_px as u32;
    let groups = (px + 15) / 16;
    let cl = rd.compute_list_begin();
    rd.compute_list_bind_compute_pipeline(cl, ctx.pipeline);
    rd.compute_list_bind_uniform_set(cl, uset, 0);
    rd.compute_list_set_push_constant(cl, &push_pba, push_pba.len() as u32);
    rd.compute_list_dispatch(cl, groups, groups, 1);
    rd.compute_list_end();
    // Free ONLY the per-page uniform set (the cached shader/pipeline/buffers persist). No
    // submit/sync — global RD auto-submits at draw. target_rid NOT freed (pool owns it).
    rd.free_rid(uset);
    Ok(())
}
```

- [ ] **Step 2: Remove the old per-page `compute_into_texture`**

Delete the `pub(crate) fn compute_into_texture(...)` function (the pool was its only caller;
Task 2 switches the pool to `compute_page_cached`). This prevents the slow path from being
reintroduced. Keep `build_page_push_constant`, `make_image_uniform`, and the
`Wg10PageCompute` class as-is (they're still used / harmless).

- [ ] **Step 3: Build**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo build`
Expected: a COMPILE ERROR in `page_pool.rs` (it still calls `compute_into_texture`) — that's
expected; Task 2 fixes the caller. The `page_compute.rs` changes themselves should compile (if
`compute_into_texture` removal leaves an unused-import, leave imports — Task 2's caller and the
new fns use them). If `page_compute.rs` has its own errors, fix them; the only EXPECTED error is
the pool's now-dangling call.

- [ ] **Step 4: Commit (after Task 2 makes it build — or commit Task 1+2 together)**

Because removing `compute_into_texture` breaks the pool until Task 2, commit Tasks 1 and 2
TOGETHER (one commit) so each commit builds. Proceed to Task 2, then commit both.

---

## Task 2: `Wg10PagePool` owns + uses the cached context

**Files:** Modify `wg-10/rust/src/page_pool.rs`

- [ ] **Step 1: Add the cached-context field + import**

In `wg-10/rust/src/page_pool.rs`, update the `use crate::page_compute::...` import to bring in
the new items, e.g.:
```rust
use crate::page_compute::{PageComputeContext, build_page_compute_context, free_page_compute_context, compute_page_cached};
```
(remove the old `compute_into_texture` import). Add a field to `Wg10PagePool`:
```rust
    compute_ctx: Option<PageComputeContext>,
```
and initialise it to `None` in `init`.

- [ ] **Step 2: Build the context in `configure`**

In `configure`, after the pack + `PackBuffers` (`pb`) + GLSL are loaded and BEFORE returning
success, build the context. `configure` runs without a guaranteed RenderingDevice in some call
orders — but the existing code obtains the global RD inside `acquire_page`. To build the context
at configure we need the RD now: get it the same way (`RenderingServer::singleton().get_rendering_device()`),
and if absent, defer (build lazily on first acquire) OR return an error. SIMPLEST + matches the
windowed-only reality: get the RD in configure; if `None`, return an error string
(configure is only meaningfully called windowed, like every pool user). Concretely, after `pb`
is built and `glsl` loaded:
```rust
        // Build the cached compute context once (shader + pipeline + the 6 pack buffers) so
        // per-page production never recompiles/re-uploads (slice 7). Needs the global RD.
        let mut rd = match RenderingServer::singleton().get_rendering_device() {
            Some(r) => r,
            None => return GString::from("configure: global RenderingDevice unavailable (windowed-only)"),
        };
        let ctx = match build_page_compute_context(&mut rd, &pb, &glsl) {
            Ok(c) => c,
            Err(e) => return GString::from(&format!("compute context: {e}")),
        };
        self.compute_ctx = Some(ctx);
```
Place this so `self.compute_ctx` is set alongside the other `self.pack`/`self.pack_buffers`/
`self.glsl_source` assignments. (Keep storing pack/pack_buffers/glsl — `compute_page_cached`
needs `grammar_constants` + `num_palettes` from them per call.)

- [ ] **Step 3: Use `compute_page_cached` in both acquire arms**

In `acquire_page`, the `Decision::Allocate` and `Decision::AllocateEvicting` arms currently call
`compute_into_texture(&mut rd, self.pack..., self.pack_buffers..., tex_rid, &glsl, ox, oz, ws, ppx, sd)`.
Replace BOTH with the cached dispatch. The cached call needs `grammar_constants` + `num_palettes`
(from the pack / pack_buffers) instead of the whole pack + glsl:
```rust
                let ctx = self.compute_ctx.as_ref().unwrap();
                let pack = self.pack.as_ref().unwrap();
                let num_palettes = self.pack_buffers.as_ref().unwrap().num_palettes;
                let result = compute_page_cached(
                    &mut rd, ctx, &pack.grammar_constants, num_palettes,
                    tex_rid, ox, oz, ws, ppx, sd,
                );
```
(`ws` is the per-level span local already computed in `acquire_page` from slice 5a; `ox/oz/ppx/sd`
unchanged. Drop the now-unused `glsl` clone in these arms if present.) The failure handling
(rollback + free on `Err`) stays exactly as it is — `compute_page_cached` returns the same
`Result<(), String>`.

- [ ] **Step 4: Free the context in `free_all`**

In `free_all`, after freeing the page textures (and regardless of the windowed-mode early
return), free the cached context if present:
```rust
        if let Some(ctx) = self.compute_ctx.take() {
            // rd available in this branch (we have textures to free); reuse the same rd handle.
            free_page_compute_context(&mut rd, &ctx);
        }
```
Place it where `rd` is in scope (the branch that has the RenderingDevice). In the windowed-mode
early-return branch (no RD), just `self.compute_ctx = None;` (nothing to free). Ensure no path
leaves a context referencing a freed RD.

- [ ] **Step 5: Build + full cargo suite**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo build` (clean) then `... cargo test`
(expect 103 passed — no unit test exercises the GPU path; this is windowed-gated).

- [ ] **Step 6: Commit (Tasks 1 + 2 together — each commit builds)**

```powershell
git add wg-10/rust/src/page_compute.rs wg-10/rust/src/page_pool.rs
git commit -m "feat(m3): cache page-compute resources in Wg10PagePool (slice 7 — the p99 fix)

The 90ms page-compute spike was redundant per-page CPU setup: recompiling the GLSL->SPIRV
shader + re-uploading all 6 pack buffers (incl. the ~25MB kernel atlas) EVERY page. Hoist them
into a PageComputeContext (shader+pipeline+6 buffer RIDs) built ONCE at configure (build_page_
compute_context) and freed at free_all (free_page_compute_context) — the pool stays the single
RID owner. Per-page production (compute_page_cached) now only builds a uniform set (cached
buffers + the page's target image) + push constant + dispatch (fire-and-forget), then frees the
per-page uniform set. No recompile, no buffer re-upload. Removed the old per-page
compute_into_texture. Zero scheduler/view/rings/shader change; page content identical (same
shader/buffers/push constant). 103 cargo tests green.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: gate `compute_ms_max` ceiling + re-measure p99 (CONTROLLER runs windowed)

**Files:** Modify `wg-10/worldgen_terrain/tests/m3_accept_check.gd`

- [ ] **Step 1: Add the compute-frame ceiling assertion**

In `m3_accept_check.gd`, after the diagnostic split is computed (`compute_ms_max` exists), add a
constant near the others and an assertion alongside the p99/stall checks:
```gdscript
const COMPUTE_CEIL_MS := 6.0   # a frame that computes a page must also fit the budget (caching
                                # eliminated the 90ms per-page rebuild; a regression would blow this)
```
and after the existing p99/max asserts:
```gdscript
	if compute_ms_max > COMPUTE_CEIL_MS:
		errs.append("compute-frame spike %.2f ms > %.1f ms (per-page rebuild regressed? caching broken)" % [compute_ms_max, COMPUTE_CEIL_MS])
```
(Keep the existing `p99 < 6ms`, no-black, `max < 33ms` asserts and the printed diagnostic line.)

- [ ] **Step 2 (CONTROLLER): build + run m3, read the numbers, VALIDATE sane**
```powershell
$env:CARGO_TARGET_DIR=$null; Push-Location wg-10/rust; cargo build; Pop-Location
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
python tools/gate.py --suite m3
```
Expected: `[wg10-m3-accept] p99=<low>ms ... compute_ms_max=<low>` and `status=pass`, `[gate]
suite=m3 checks=5 fail=0`. Validate the numbers are sane: render-only ≤ ~2ms (unchanged),
compute_ms_max now low single-digit ms (was 90), p99 < 6ms. If compute_ms_max is STILL high, the
caching didn't take (or a residual genuinely-expensive per-page cost remains) — diagnose (is the
context actually built once? is a buffer still re-uploaded?); if a real residual remains, that's
the async follow-up with a precise number. Do NOT raise the ceilings to pass.

- [ ] **Step 3 (CONTROLLER): fast + gpu + the other m3 checks unaffected**
```powershell
python tools/gate.py --suite fast
python tools/gate.py --suite gpu
```
Expected: fast 5 / gpu 2 fail=0; m3 slice1/pool/stream/view still pass (page content identical).

- [ ] **Step 4: Commit**
```powershell
git add wg-10/worldgen_terrain/tests/m3_accept_check.gd
git commit -m "test(m3): m3_accept_check compute-frame ceiling; p99 GREEN after caching

Add compute_ms_max < 6ms assertion (locks in the caching win: a regression to per-page rebuild
would blow it again). With slice-7 caching, the gate now passes: p99=<measured>ms (was 16.7),
compute_ms_max=<measured>ms (was 90), render-only <=2ms, no-black, never-stall. m3 suite 5 checks
fail=0. The render pipeline now meets the p99<6ms budget at ~1000 m/s.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: STATUS + ROADMAP — p99 GREEN; M3 open only for the owner's manual fly

**Files:** Modify `docs/plans/STATUS.md`, `docs/plans/ROADMAP.md`

- [ ] **Step 1: STATUS.md** — `Last updated:` + replace the "p99 RED / async-next" framing with:
  caching landed; `m3_accept_check` GREEN (record the actual p99 + compute_ms_max); the async
  item marked NOT NEEDED (caching resolved the spike) — or, if a residual remained, re-scoped with
  the number. M3 milestone has ONE box left: the owner's manual fly of `m3_review.tscn`. Gate-
  runner line: m3 = 5 checks fail=0.

- [ ] **Step 2: ROADMAP.md** — `Last updated:`; flip the acceptance-gate item `[~]`→`[x]` (p99
  gate green, numbers); the async item → DONE-NOT-NEEDED-via-caching (or re-scoped); the MANUAL
  ACCEPTANCE box stays `[ ]` (owner's fly).

- [ ] **Step 3: fresh evidence** — copy ACTUAL numbers (`cargo test` result line + the gate's
  `p99=.. compute_ms_max=..` + `suite=m3 checks=5 fail=0`).

- [ ] **Step 4: Commit**
```powershell
git add docs/plans/STATUS.md docs/plans/ROADMAP.md
git commit -m "docs(m3): page-compute caching done; p99 gate GREEN; M3 open only for the manual fly

Caching eliminated the per-page rebuild spike: m3_accept_check now p99=<n>ms (budget 6),
compute_ms_max=<n>ms (was 90), render-only <=2ms. m3 suite 5 checks fail=0. Async page
production NOT needed (caching resolved it). M3 milestone has one remaining box: the owner's
manual fly of m3_review.tscn (the final authority, §7.3).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage:** §2 PageComputeContext + build/free → Task 1. §3.1 build at configure →
Task 2 Step 2. §3.2 cached dispatch in both acquire arms → Task 2 Step 3. §3.3 free at free_all →
Task 2 Step 4. §3.4 remove old compute_into_texture → Task 1 Step 2. §4 gate compute_ms_max
ceiling + p99 green → Task 3. §6 done (single-owner, content-identical, full-suite regression) →
Tasks 2/3. ✓

**2. Placeholder scan:** No TBD/"handle edge cases". Complete code in every code step. The gate's
"validate sane / don't raise ceilings / residual→async" is an explicit controller instruction,
not a placeholder. The configure-needs-RD point is resolved concretely (get the global RD in
configure; error if absent — windowed-only reality). ✓

**3. Type consistency:** `PageComputeContext` fields (shader/pipeline/palettes/compat_off/
compat_flat/krec/kparam/kdata) consistent across build/free/dispatch (Task 1) and the pool field
(Task 2). `build_page_compute_context(rd, pb, glsl) -> Result<PageComputeContext,String>`,
`free_page_compute_context(rd, &ctx)`, `compute_page_cached(rd, &ctx, &gc, num_palettes,
target_rid, ox, oz, ws, ppx, sd) -> Result<(),String>` — signatures consistent Task 1 (def) ↔
Task 2 (calls). `num_palettes` from `pack_buffers.num_palettes` (matches the existing
`build_page_push_constant` arg the old code passed as `pb.num_palettes`). `grammar_constants` from
`pack.grammar_constants` (the old code passed `&pack.grammar_constants`). `make_image_uniform`/
`make_storage_uniform`/`bytes_to_pba`/`build_page_push_constant` reused with their existing
signatures. ✓

**Note:** `num_palettes` — the old `compute_into_texture` derived it from `pb.num_palettes` (the
PackBuffers field). Task 1's `compute_page_cached` takes it as a param; Task 2 passes
`self.pack_buffers.as_ref().unwrap().num_palettes`. Confirm `PackBuffers` has a `num_palettes:
i32` field (the old push-constant call used `pb.num_palettes`) — it does (slice 2). ✓
