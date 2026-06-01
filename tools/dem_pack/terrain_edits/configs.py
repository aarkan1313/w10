from __future__ import annotations
import terrain_edits as te
import terrain_edits.placement as pl
import terrain_edits.profile as pr


def mountain_trail() -> te.TerrainEdit:
    """Sparse thin Fellowship-style mountain trails: valley-following route + thin climbing ledge."""
    return te.TerrainEdit(
        placement=pl.low_corridor_route, placement_params=pl.LowCorridorParams(low_pref=8.0, route_count=1),
        axes=("x", "z"),
        profile=pr.thin_climbing_trail, profile_params=pr.ThinTrailParams(),
    )


# --- SKETCHES: prove the abstraction holds vs >1 use. Real placements/profiles land when each is built. ---

def road() -> te.TerrainEdit:
    """Sketch: a flat-ish path. Uses low_corridor_route + a wide gentle trail as a stand-in road bed."""
    return te.TerrainEdit(
        placement=pl.low_corridor_route, placement_params=pl.LowCorridorParams(low_pref=4.0),
        axes=("x",), profile=pr.graded_valley, profile_params=pr.GradedValleyParams(floor_grade_frac=0.2, trail_width_m=600.0))


def river() -> te.TerrainEdit:
    """Sketch: an incised channel. Stand-in = a narrow deep graded valley along low ground."""
    return te.TerrainEdit(
        placement=pl.low_corridor_route, placement_params=pl.LowCorridorParams(low_pref=12.0),
        axes=("x",), profile=pr.graded_valley, profile_params=pr.GradedValleyParams(trail_width_m=300.0, depth_cap_m=400.0))


def lake() -> te.TerrainEdit:
    """Sketch: a basin fill. Stand-in = a wide shallow graded valley (placeholder for basin_fill+lake_surface)."""
    return te.TerrainEdit(
        placement=pl.low_corridor_route, placement_params=pl.LowCorridorParams(low_pref=12.0),
        axes=("x",), profile=pr.graded_valley, profile_params=pr.GradedValleyParams(trail_width_m=2000.0, depth_cap_m=300.0))


def poi() -> te.TerrainEdit:
    """Sketch: a level pad. Stand-in = a short wide flat trail (placeholder for point+level_pad)."""
    return te.TerrainEdit(
        placement=pl.low_corridor_route, placement_params=pl.LowCorridorParams(low_pref=2.0),
        axes=("x",), profile=pr.graded_valley, profile_params=pr.GradedValleyParams(floor_grade_frac=0.1, trail_width_m=800.0))
