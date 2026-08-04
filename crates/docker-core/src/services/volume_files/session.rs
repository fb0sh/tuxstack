//! Session handle types and label constants for volume preview helpers.

use crate::models::VolumePreviewSession;

pub const LABEL_MANAGED: &str = "io.github.tuxstack.managed";
pub const LABEL_PURPOSE: &str = "io.github.tuxstack.purpose";
pub const LABEL_VOLUME: &str = "io.github.tuxstack.volume";
pub const LABEL_SESSION: &str = "io.github.tuxstack.session";
pub const PURPOSE_VALUE: &str = "volume-preview";

/// Convenience alias kept for callers that prefer a handle name.
pub type VolumePreviewSessionHandle = VolumePreviewSession;
