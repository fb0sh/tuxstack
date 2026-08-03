//! Image domain models.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A short list entry for an image.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageSummary {
    pub id: String,
    pub short_id: String,
    pub repository_tags: Vec<String>,
    pub repository_digests: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub size_bytes: u64,
    pub virtual_size_bytes: u64,
    pub containers: u64,
    pub labels: BTreeMap<String, String>,
}

impl ImageSummary {
    /// The primary tag, falling back to `<none>`.
    pub fn primary_tag(&self) -> &str {
        self.repository_tags
            .first()
            .map(|s| s.as_str())
            .unwrap_or("<none>")
    }
}
