//! Docker event domain model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A Docker engine event (streamed from `/events`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockerEvent {
    pub event_type: String,
    pub action: String,
    pub actor_id: Option<String>,
    pub actor_attributes: Vec<(String, String)>,
    pub time: Option<DateTime<Utc>>,
}
