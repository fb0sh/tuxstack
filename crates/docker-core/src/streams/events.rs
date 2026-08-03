//! Docker engine event stream.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use futures_util::{Stream, StreamExt};
use tokio_util::sync::CancellationToken;

use crate::client::DockerClient;
use crate::error::{classify_api_error, DockerError};
use crate::mapping::system::map_event;
use crate::models::DockerEvent;

pub type EventStreamResult =
    Pin<Box<dyn Stream<Item = Result<DockerEvent, DockerError>> + Send>>;

/// Event stream service.
#[derive(Clone)]
pub struct EventStream {
    client: Arc<DockerClient>,
}

impl EventStream {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { client }
    }

    /// Stream Docker engine events.
    ///
    /// The stream ends when the token is cancelled or the engine
    /// disconnects (returning a final error).
    pub fn watch_events(
        &self,
        cancel: CancellationToken,
    ) -> EventStreamResult {
        let docker = self.client.inner().clone();
        let filters: HashMap<String, Vec<String>> = HashMap::new();
        let opts = bollard::query_parameters::EventsOptions {
            since: None,
            until: None,
            filters: if filters.is_empty() { None } else { Some(filters) },
        };

        let inner = docker.events(Some(opts)).map(|item| match item {
            Ok(event) => Ok(map_event(event)),
            Err(e) => Err(classify_api_error(&e, "system")),
        });
        Box::pin(inner.take_until(cancel.cancelled_owned())).boxed()
    }
}
