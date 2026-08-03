//! Compose project model.
//!
//! Compose support is planned but not implemented in the current alpha.
//! The model exists so the GUI can render an honest "planned" state
//! without fake data.

use serde::{Deserialize, Serialize};

/// A Docker Compose project (planned feature).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposeProject {
    pub name: String,
    pub config_files: Vec<String>,
    pub status: ComposeProjectStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposeProjectStatus {
    Running,
    Partial,
    Stopped,
    Unknown,
}
