//! Wg10ClipmapRings (DESIGN §5.1) — the godot owner of the clipmap ring meshes. Each level
//! is a 3x3 neighborhood of one-page TILES (9 MeshInstance3D per level) so the level
//! surrounds the camera. Each tile is a full grid spanning one page, with its own
//! ShaderMaterial(ring_displace). Levels OVERLAP: the coarse level keeps its full 3x3, the
//! finer level's 3x3 draws on top (render_priority by level), and the geomorph blends at the
//! finer's outer edge — gapless by construction. `bind_tile` places + binds one tile each
//! frame. Persistent: tiles are created once, never rebuilt (only transform + uniforms
//! change). Owns NO page RIDs — it only samples textures the pool owns.

use crate::ring_geometry::{band_mesh, RingLayout, RingMesh};
use godot::classes::{
    mesh::{ArrayType, PrimitiveType},
    ArrayMesh, INode3D, MeshInstance3D, Shader, ShaderMaterial,
};
use godot::prelude::*;
use std::time::Instant;

/// Tiles per level: a 3x3 neighborhood (radius 1).
const TILES_PER_LEVEL: usize = 9;

/// Wall-clock height fade for newly-bound pages. The fade starts from the parent page's height
/// and ramps to the newly resident fine page, hiding repage pop-in without adding page work.
/// Keep this short in the owner fly: the previous 0.18s window read as terrain lagging/settling
/// during motion across modes 1/2/3.
const PAGE_FADE_SECONDS: f32 = 0.06;

/// Custom-AABB Y half-height (metres) for GPU-displaced tiles. The shader moves VERTEX.y, so each
/// tile's real vertical extent must be declared to Godot's frustum culler or tiles vanish when
/// their flat (y=0) box leaves the frustum. Generous enough for worst-case z-score DEM heights
/// times any reasonable relief_scale; over-sizing only slightly loosens culling (negligible).
const DISPLACE_AABB_HALF_M: f32 = 8000.0;

/// Flat tile index from (level, dx, dz) with dx,dz in {-1,0,+1}.
fn tile_index(level: i32, dx: i32, dz: i32) -> usize {
    (level as usize) * TILES_PER_LEVEL + ((dz + 1) as usize) * 3 + (dx + 1) as usize
}

#[derive(GodotClass)]
#[class(base=Node3D)]
pub struct Wg10ClipmapRings {
    tiles: Vec<Gd<MeshInstance3D>>,
    bound_keys: Vec<(i64, i64)>,
    fade_values: Vec<f32>,
    fade_last_update: Vec<Instant>,
    num_levels: i32,
    base_span: f64,
    grid_res: i32,
    built_vertex_count: i64,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for Wg10ClipmapRings {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            tiles: Vec::new(),
            bound_keys: Vec::new(),
            fade_values: Vec::new(),
            fade_last_update: Vec::new(),
            num_levels: 0,
            base_span: 0.0,
            grid_res: 0,
            built_vertex_count: 0,
            base,
        }
    }
}

#[godot_api]
impl Wg10ClipmapRings {
    #[func]
    pub fn configure(
        &mut self,
        num_levels: i64,
        base_span: f64,
        grid_res: i64,
        shader_path: GString,
    ) {
        if !self.tiles.is_empty() {
            godot_error!("Wg10ClipmapRings::configure called more than once — ignoring");
            return;
        }
        if grid_res < 1 || grid_res % 4 != 0 {
            godot_error!(
                "Wg10ClipmapRings: grid_res must be >= 1 and divisible by 4, got {grid_res}"
            );
            return;
        }
        self.num_levels = num_levels as i32;
        self.base_span = base_span;
        self.grid_res = grid_res as i32;
        self.built_vertex_count = 0;

        let shader: Gd<Shader> = match try_load::<Shader>(&shader_path) {
            Ok(s) => s,
            Err(_) => {
                godot_error!("Wg10ClipmapRings: failed to load shader {shader_path}");
                return;
            }
        };

        let total = (self.num_levels as usize) * TILES_PER_LEVEL;
        self.bound_keys = vec![(i64::MIN, i64::MIN); total];
        self.fade_values = vec![1.0; total];
        self.fade_last_update = vec![Instant::now(); total];

        for level in 0..self.num_levels {
            let priority = (self.num_levels - 1 - level) as i32; // finest (0) -> highest -> on top
            let span_l = self.base_span * 2f64.powi(level);
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let tile_layout = RingLayout::new(1, span_l); // full grid at span_l
                    let rm: RingMesh = band_mesh(&tile_layout, 0, self.grid_res);
                    self.built_vertex_count += rm.positions.len() as i64;
                    let mesh = build_array_mesh(&rm);

                    let mut mi = MeshInstance3D::new_alloc();
                    mi.set_mesh(&mesh);
                    let mut mat = ShaderMaterial::new_gd();
                    mat.set_shader(&shader);
                    mat.set_render_priority(priority);
                    mi.set_material_override(&mat);
                    // The shader displaces VERTEX.y on the GPU, so the mesh's real bounds are
                    // taller than its flat (y=0) geometry. Godot frustum-culls on the AABB — without
                    // a TALL custom AABB a tile whose displaced terrain is on-screen gets culled when
                    // its flat box leaves the frustum (tiles vanish on rotation / slow creep near the
                    // view edge). Set a custom AABB spanning the tile's full XZ footprint + a generous
                    // Y range covering worst-case displacement (z-score DEM heights * relief_scale).
                    let half = (span_l * 0.5) as f32;
                    let y_half = DISPLACE_AABB_HALF_M;
                    mi.set_custom_aabb(Aabb::new(
                        Vector3::new(-half, -y_half, -half),
                        Vector3::new(span_l as f32, y_half * 2.0, span_l as f32),
                    ));
                    self.base_mut().add_child(&mi);
                    self.tiles.push(mi);
                    let _ = (dx, dz);
                }
            }
        }
    }

    #[func]
    pub fn level_count(&self) -> i64 {
        self.num_levels as i64
    }

    #[func]
    pub fn tile_count(&self) -> i64 {
        self.tiles.len() as i64
    }

    #[func]
    pub fn total_vertex_count(&self) -> i64 {
        let mut total = 0i64;
        let mut read_any = false;
        for mi in &self.tiles {
            if let Some(mesh) = mi.get_mesh() {
                if let Ok(am) = mesh.try_cast::<ArrayMesh>() {
                    if am.get_surface_count() > 0 {
                        let arrays = am.surface_get_arrays(0);
                        let verts: PackedVector3Array = arrays.at(0).to();
                        total += verts.len() as i64;
                        read_any = true;
                    }
                }
            }
        }
        if read_any {
            total
        } else {
            self.built_vertex_count
        }
    }

    #[func]
    pub fn bound_page_key(&self, level: i64, dx: i64, dz: i64) -> Vector2i {
        let idx = tile_index(level as i32, dx as i32, dz as i32);
        if idx >= self.bound_keys.len() {
            return Vector2i::new(i32::MIN, i32::MIN);
        }
        let (ox, oz) = self.bound_keys[idx];
        Vector2i::new(ox as i32, oz as i32)
    }

    #[func]
    pub fn bind_tile(
        &mut self,
        level: i64,
        dx: i64,
        dz: i64,
        height_tex: Gd<godot::classes::Texture2D>,
        coarse_tex: Gd<godot::classes::Texture2D>,
        tile_origin: Vector2,
        // spans packs (sample_span, coarse_span); placement packs (tile_span, level_half_extent)
        // — folded into Vector2 to stay under gdext's #[func] param-arity cap (max 15 args).
        spans: Vector2,
        placement: Vector2,
        relief_scale: f64,
        morph_region: f64,
        relief_ref: f64,
        sample_origin: Vector2,
        coarse_origin: Vector2,
        level_center: Vector2,
    ) {
        let sample_span = spans.x as f64;
        let coarse_span = spans.y as f64;
        let tile_span = placement.x as f64;
        let level_half_extent = placement.y as f64;
        let next_key = (sample_origin.x as i64, sample_origin.y as i64);
        let idx = tile_index(level as i32, dx as i32, dz as i32);
        if idx >= self.tiles.len() {
            godot_error!("Wg10ClipmapRings::bind_tile: ({level},{dx},{dz}) out of range");
            return;
        }
        let now = Instant::now();
        let was_hidden = !self.tiles[idx].is_visible();
        if self.bound_keys[idx] != next_key || was_hidden {
            self.fade_values[idx] = 0.0;
        } else {
            let dt = now.duration_since(self.fade_last_update[idx]).as_secs_f32();
            self.fade_values[idx] = (self.fade_values[idx] + dt / PAGE_FADE_SECONDS).min(1.0);
        }
        self.fade_last_update[idx] = now;
        // Placement is INVARIANT — tile (level,dx,dz) always sits in its fine grid slot,
        // covering world [tile_origin, tile_origin+tile_span], whatever it samples. Sampling
        // (sample_origin/sample_span = the fine page, OR the coarse page on fallback) is set
        // separately on the material so a fallback tile reads the coarse page in the coarse
        // page's OWN uv frame (the slice-8 flicker was placement & sampling sharing one origin).
        {
            let mi = &mut self.tiles[idx];
            let mut t = mi.get_transform();
            t.origin = Vector3::new(
                tile_origin.x + (tile_span * 0.5) as f32,
                0.0,
                tile_origin.y + (tile_span * 0.5) as f32,
            );
            mi.set_transform(t);
        }
        let mi = &mut self.tiles[idx];
        let Some(mat_res) = mi.get_material_override() else {
            godot_error!("Wg10ClipmapRings::bind_tile: tile has no material");
            return;
        };
        let Ok(mut mat) = mat_res.try_cast::<ShaderMaterial>() else {
            godot_error!("Wg10ClipmapRings::bind_tile: material is not a ShaderMaterial");
            return;
        };
        mat.set_shader_parameter("height_tex", &height_tex.to_variant());
        mat.set_shader_parameter("coarse_height_tex", &coarse_tex.to_variant());
        mat.set_shader_parameter("static_material_tex", &height_tex.to_variant());
        mat.set_shader_parameter("static_material_mix", &0.0_f64.to_variant());
        mat.set_shader_parameter("world_span", &sample_span.to_variant());
        mat.set_shader_parameter("coarse_span", &coarse_span.to_variant());
        mat.set_shader_parameter("relief_scale", &relief_scale.to_variant());
        mat.set_shader_parameter("morph_region", &morph_region.to_variant());
        mat.set_shader_parameter("relief_ref", &relief_ref.to_variant());
        mat.set_shader_parameter("coarse_origin", &coarse_origin.to_variant());
        mat.set_shader_parameter("page_fade", &self.fade_values[idx].to_variant());
        // fine SAMPLE frame (world-UV: the fine page, or the coarse page on fallback) + the
        // level's 3x3 neighborhood center & half-extent (geomorph engages at the outer ring only).
        mat.set_shader_parameter("page_origin", &sample_origin.to_variant());
        mat.set_shader_parameter("level_center", &level_center.to_variant());
        mat.set_shader_parameter("level_half_extent", &level_half_extent.to_variant());
        self.tiles[idx].set_visible(true); // binding a tile implies it is shown
        self.bound_keys[idx] = next_key;
    }

    /// Set the per-tile biome presentation color. `wg_dbg_mode == 2` shows it directly.
    /// Normal material mode can also use `material_mix` as a restrained tint. Both are
    /// presentation-only; they do not affect page data, producer routing, or facts.
    #[func]
    pub fn set_tile_debug_color(
        &mut self,
        level: i64,
        dx: i64,
        dz: i64,
        color: Color,
        material_mix: f64,
    ) {
        let idx = tile_index(level as i32, dx as i32, dz as i32);
        if idx >= self.tiles.len() {
            godot_error!(
                "Wg10ClipmapRings::set_tile_debug_color: ({level},{dx},{dz}) out of range"
            );
            return;
        }
        let Some(mat_res) = self.tiles[idx].get_material_override() else {
            godot_error!("Wg10ClipmapRings::set_tile_debug_color: tile has no material");
            return;
        };
        let Ok(mut mat) = mat_res.try_cast::<ShaderMaterial>() else {
            godot_error!(
                "Wg10ClipmapRings::set_tile_debug_color: material is not a ShaderMaterial"
            );
            return;
        };
        mat.set_shader_parameter("biome_debug_color", &color.to_variant());
        mat.set_shader_parameter("biome_material_mix", &material_mix.to_variant());
    }

    #[func]
    pub fn set_tile_static_material(
        &mut self,
        level: i64,
        dx: i64,
        dz: i64,
        material_tex: Gd<godot::classes::Texture2D>,
        material_mix: f64,
    ) {
        let idx = tile_index(level as i32, dx as i32, dz as i32);
        if idx >= self.tiles.len() {
            godot_error!(
                "Wg10ClipmapRings::set_tile_static_material: ({level},{dx},{dz}) out of range"
            );
            return;
        }
        let Some(mat_res) = self.tiles[idx].get_material_override() else {
            godot_error!("Wg10ClipmapRings::set_tile_static_material: tile has no material");
            return;
        };
        let Ok(mut mat) = mat_res.try_cast::<ShaderMaterial>() else {
            godot_error!(
                "Wg10ClipmapRings::set_tile_static_material: material is not a ShaderMaterial"
            );
            return;
        };
        mat.set_shader_parameter("static_material_tex", &material_tex.to_variant());
        mat.set_shader_parameter("static_material_mix", &material_mix.to_variant());
    }

    /// Drop every tile material's reference to the pool's page textures, and hide all tiles.
    ///
    /// MUST be called BEFORE the pool's `free_all()` whenever the page textures are about to be
    /// freed (scene teardown OR a live `free_all`+reconfigure, e.g. the relief/A-B toggles in the
    /// review scene). Otherwise the tile materials still bind the now-freed page-texture RIDs at
    /// `height_tex`/`coarse_height_tex` (binding 0/1), and the NEXT draw rebuilds each material's
    /// render uniform set against a freed texture -> "Texture (binding 1) is not a valid texture" +
    /// "uniform_set_set_invalidation_callback: us is null", once per page that was bound as a coarse
    /// texture. The page textures are valid during the whole fly (proven: a per-bind validity probe
    /// saw zero invalids); the ONLY time binding 1 is invalid is AFTER free_all, which is exactly
    /// what this prevents. Clearing the params to a NIL Variant detaches the Texture2DRD so the
    /// material holds no dangling page reference, and hiding the tile keeps the never-black contract
    /// (the next configured page re-binds + re-shows it via `bind_tile`).
    #[func]
    pub fn unbind_all(&mut self) {
        for mi in self.tiles.iter_mut() {
            if let Some(mat_res) = mi.get_material_override() {
                if let Ok(mut mat) = mat_res.try_cast::<ShaderMaterial>() {
                    // Detach the page textures (NIL Variant => the sampler param holds no texture),
                    // so this material no longer references a soon-to-be-freed page RID.
                    mat.set_shader_parameter("height_tex", &Variant::nil());
                    mat.set_shader_parameter("coarse_height_tex", &Variant::nil());
                    mat.set_shader_parameter("static_material_tex", &Variant::nil());
                    mat.set_shader_parameter("static_material_mix", &0.0_f64.to_variant());
                }
            }
            mi.set_visible(false);
        }
        for k in self.bound_keys.iter_mut() {
            *k = (i64::MIN, i64::MIN);
        }
        for f in self.fade_values.iter_mut() {
            *f = 1.0;
        }
        let now = Instant::now();
        for t in self.fade_last_update.iter_mut() {
            *t = now;
        }
    }

    /// Show/hide one tile. The view HIDES a tile whose own level page is not resident, so the
    /// coarser level's full 3x3 (drawn underneath, lower render_priority) shows through — the
    /// never-black blanket. A bound tile is shown; an unready one is hidden, NEVER left at a
    /// stale position (that was the slice-8 "stuff overlaid on stuff" bug).
    #[func]
    pub fn set_tile_visible(&mut self, level: i64, dx: i64, dz: i64, visible: bool) {
        let idx = tile_index(level as i32, dx as i32, dz as i32);
        if idx >= self.tiles.len() {
            godot_error!("Wg10ClipmapRings::set_tile_visible: ({level},{dx},{dz}) out of range");
            return;
        }
        self.tiles[idx].set_visible(visible);
    }

    /// DEBUG: per-tile current state as a flat array, 3 ints per tile in tile-index order:
    /// [visible(0/1), bound_ox, bound_oz]. Lets the review scene detect & log flips
    /// (HIDE/SHOW/REPAGE) so a vanishing chunk in the live fly names its own (level, slot) + cause
    /// — the residency/coverage probes all show 0 holes, so the bug is in render/visibility, and
    /// this surfaces exactly which tile drops and why.
    #[func]
    pub fn debug_tile_states(&self) -> PackedInt64Array {
        let mut out = PackedInt64Array::new();
        for (idx, mi) in self.tiles.iter().enumerate() {
            out.push(if mi.is_visible() { 1 } else { 0 });
            let (ox, oz) = self.bound_keys[idx];
            out.push(ox);
            out.push(oz);
        }
        out
    }

    /// DEBUG: force every tile's frustum culling off (`set_extra_cull_margin` huge) so a tile is
    /// NEVER culled regardless of its AABB. If a "vanishing chunk" STOPS with this on, the cause is
    /// frustum culling (AABB); if it persists, the cause is the bind/visibility path. A/B switch.
    #[func]
    pub fn debug_disable_culling(&mut self, disabled: bool) {
        let margin: f32 = if disabled { 1.0e9 } else { 0.0 };
        for mi in self.tiles.iter_mut() {
            mi.set_extra_cull_margin(margin);
        }
    }
}

fn build_array_mesh(rm: &RingMesh) -> Gd<ArrayMesh> {
    let mut verts = PackedVector3Array::new();
    for v in &rm.positions {
        verts.push(Vector3::new(v.x as f32, v.y as f32, v.z as f32));
    }
    let mut indices = PackedInt32Array::new();
    for i in &rm.indices {
        indices.push(*i as i32);
    }
    let mut arrays = VarArray::new();
    arrays.resize(ArrayType::MAX.ord() as usize, &Variant::nil());
    arrays.set(ArrayType::VERTEX.ord() as usize, &verts.to_variant());
    arrays.set(ArrayType::INDEX.ord() as usize, &indices.to_variant());
    let mut mesh = ArrayMesh::new_gd();
    mesh.add_surface_from_arrays(PrimitiveType::TRIANGLES, &arrays);
    mesh
}
