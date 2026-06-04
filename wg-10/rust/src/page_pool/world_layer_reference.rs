//! Accepted world-layer references bound beside live producers.
//!
//! This is distinct from `static_ref`, which is itself the active page
//! producer. A bound reference is a bridge/fact source owned by another active
//! producer such as live MOUNTAIN or WORLD preview.

use std::path::Path;

use super::StaticHeightRuntime;

#[derive(Clone)]
pub(super) struct BoundWorldLayerReference {
    reference: StaticHeightRuntime,
    source_transform: SourceDisplayTransform,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SourceDisplayTransform {
    pub(super) scale: f64,
    pub(super) offset_x_m: f64,
    pub(super) offset_z_m: f64,
}

impl BoundWorldLayerReference {
    pub(super) fn from_json_path(path: &Path) -> Result<Self, String> {
        let reference = StaticHeightRuntime::from_json_path(path)?;
        let (scale, offset_x_m, offset_z_m) = reference.source_transform_for_display()?;
        Ok(Self {
            reference,
            source_transform: SourceDisplayTransform {
                scale,
                offset_x_m,
                offset_z_m,
            },
        })
    }

    pub(super) fn reference(&self) -> &StaticHeightRuntime {
        &self.reference
    }

    pub(super) fn source_transform(&self) -> SourceDisplayTransform {
        self.source_transform
    }
}
