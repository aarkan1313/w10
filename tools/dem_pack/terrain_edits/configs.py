from __future__ import annotations
import terrain_edits as te
import terrain_edits.placement as pl
import terrain_edits.profile as pr


def mountain_trail(route_count: int = 1) -> te.TerrainEdit:
    """Sparse thin Fellowship-style mountain trails: valley-following route + thin climbing ledge.
    route_count > 1 spreads that many crossings per axis across the range (less untouched dead area;
    too many starts to read as an artificial grid -- tune by eye)."""
    return te.TerrainEdit(
        placement=pl.low_corridor_route, placement_params=pl.LowCorridorParams(low_pref=8.0, route_count=int(route_count)),
        axes=("x", "z"),
        profile=pr.thin_climbing_trail, profile_params=pr.ThinTrailParams(),
    )


def mountain_trail_connected() -> te.TerrainEdit:
    """FULL-TRAVERSAL mountain trail: 4 thin arms from a central meeting waypoint out to each edge (W/E/N/S),
    sharing the waypoint -> ONE connected network you can traverse fully left<->right AND up<->down, meeting in
    the middle. Use when the game needs guaranteed whole-map crossability (vs the sparse single-pass default)."""
    return te.TerrainEdit(
        placement=pl.cross_waypoint, placement_params=pl.CrossWaypointParams(low_pref=8.0, center_frac=0.5),
        axes=("x",),   # cross_waypoint defines its own 4-arm geometry; axis is ignored
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
