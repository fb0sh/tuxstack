//! Volume domain models.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A short list entry for a volume.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VolumeSummary {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub scope: String,
    pub created_at: Option<DateTime<Utc>>,
    pub labels: BTreeMap<String, String>,
    pub options: BTreeMap<String, String>,
}
