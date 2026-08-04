//! Docker Engine event stream backed by Bollard.
//!
//! Keeping the stream on the shared Bollard client preserves Unix socket,
//! remote endpoint, authentication, API-version, and cancellation semantics in
//! one place. No Docker CLI or hand-written HTTP/chunk parser is involved.

use std::pin::Pin;
use std::sync::Arc;

use futures_util::{Stream, StreamExt};
use tokio_util::sync::CancellationToken;

use crate::client::DockerClient;
use crate::error::{DockerError, classify_api_error};
use crate::mapping::system::map_event;
use crate::models::DockerEvent;

pub type EventStreamResult = Pin<Box<dyn Stream<Item = Result<DockerEvent, DockerError>> + Send>>;

/// Event stream service using the same client as every other Docker service.
#[derive(Clone)]
pub struct EventStream {
    client: Arc<DockerClient>,
}

impl EventStream {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { client }
    }

    /// Stream all Docker Engine events until the token is cancelled or the
    /// connection ends. Reconnection and debounce are owned by
    /// `DockerEventMonitor`, not this transport wrapper.
    pub fn watch_events(&self, cancel: CancellationToken) -> EventStreamResult {
        let stream = self
            .client
            .inner()
            .events(None)
            .map(|event| {
                event
                    .map(map_event)
                    .map_err(|error| classify_api_error(&error, "event"))
            })
            .take_until(cancel.cancelled_owned());
        Box::pin(stream)
    }
}

#[cfg(test)]
mod tests {
    use bollard::models::{EventActor, EventMessage, EventMessageTypeEnum};

    use super::*;

    #[test]
    fn bollard_event_mapping_preserves_actor_and_action() {
        let mapped = map_event(EventMessage {
            typ: Some(EventMessageTypeEnum::CONTAINER),
            action: Some("health_status: healthy".into()),
            actor: Some(EventActor {
                id: Some("abc123".into()),
                attributes: Some(
                    [("name".to_string(), "web".to_string())]
                        .into_iter()
                        .collect(),
                ),
            }),
            time: Some(1_700_000_000),
            ..Default::default()
        });
        assert_eq!(mapped.event_type, "container");
        assert_eq!(mapped.action, "health_status: healthy");
        assert_eq!(mapped.actor_id.as_deref(), Some("abc123"));
        assert_eq!(mapped.actor_attributes[0].0, "name");
    }
}
