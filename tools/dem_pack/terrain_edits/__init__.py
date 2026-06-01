"""Tunable terrain-edit framework: edit = (Placement WHERE + Profile WHAT) -> seam-exact world-local delta.
See docs/superpowers/specs/2026-06-01-worldgen-terrain-edit-framework-design.md."""

from terrain_edits.edit import TerrainEdit, apply_edits
from terrain_edits.apply import EditContext, blend_edges, bound_depth, combine
from terrain_edits import placement, profile
