//! Image management service.

use std::collections::HashMap;
use std::sync::Arc;

use bollard::query_parameters::{
    ListImagesOptions as BollardListImagesOptions, RemoveImageOptions,
};

use crate::client::DockerClient;
use crate::error::{classify_api_error, DockerError};
use crate::mapping::images::map_image_summary;
use crate::models::ImageSummary;

/// Options for listing images.
#[derive(Debug, Clone, Default)]
pub struct ListImagesOptions {
    /// Local search filter on tags and IDs.
    pub search: Option<String>,
}

/// Image service backed by the shared Docker client.
#[derive(Clone)]
pub struct ImageService {
    client: Arc<DockerClient>,
}

impl ImageService {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { client }
    }

    /// List images, optionally filtered locally by tag/ID search.
    pub async fn list_images(
        &self,
        options: &ListImagesOptions,
    ) -> Result<Vec<ImageSummary>, DockerError> {
        let filters: HashMap<String, Vec<String>> = HashMap::new();
        let bollard_opts = BollardListImagesOptions {
            all: true,
            filters: if filters.is_empty() { None } else { Some(filters) },
            ..Default::default()
        };

        let docker = self.client.inner().clone();
        let images = tokio::time::timeout(
            self.client.config().request_timeout,
            docker.list_images(Some(bollard_opts)),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "image"))?;

        let mut mapped: Vec<ImageSummary> = images.into_iter().map(map_image_summary).collect();

        if let Some(search) = options.search.as_deref().map(str::to_lowercase) {
            mapped.retain(|i| {
                i.repository_tags.iter().any(|t| t.to_lowercase().contains(&search))
                    || i.id.contains(&search)
                    || i.short_id.contains(&search)
            });
        }

        // Sort by most recent first for a stable, useful order.
        mapped.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(mapped)
    }

    /// Inspect a single image.
    pub async fn inspect_image(
        &self,
        id_or_name: &str,
    ) -> Result<serde_json::Value, DockerError> {
        let docker = self.client.inner().clone();
        let inspect = tokio::time::timeout(
            self.client.config().request_timeout,
            docker.inspect_image(id_or_name),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "image"))?;
        // The inspect response is Docker-specific; we hand back the raw
        // JSON for the Inspect view (never to QML models directly).
        serde_json::to_value(inspect).map_err(|e| DockerError::InvalidResponse(e.to_string()))
    }

    /// Remove an image.
    pub async fn remove_image(&self, id_or_name: &str) -> Result<(), DockerError> {
        let opts = RemoveImageOptions {
            force: false,
            noprune: false,
            ..Default::default()
        };
        let docker = self.client.inner().clone();
        tokio::time::timeout(
            self.client.config().request_timeout,
            docker.remove_image(id_or_name, Some(opts), None),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "image"))?;
        Ok(())
    }
}
