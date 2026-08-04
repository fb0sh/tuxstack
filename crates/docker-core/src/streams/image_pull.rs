//! Cancellable, domain-typed Docker image pull stream.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::Stream;
use tokio_util::sync::CancellationToken;

use crate::{DockerError, ImagePullProgress};

/// Stream returned by [`crate::ImageService::pull_image`].
///
/// Dropping it cancels the client-side request. `cancel` additionally wakes a
/// task currently waiting for Docker to emit its next progress event.
pub struct ImagePullStream {
    pub(crate) inner:
        Pin<Box<dyn Stream<Item = Result<ImagePullProgress, DockerError>> + Send + 'static>>,
    pub(crate) cancel: CancellationToken,
}

impl ImagePullStream {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel.clone()
    }
}

impl Stream for ImagePullStream {
    type Item = Result<ImagePullProgress, DockerError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

impl Drop for ImagePullStream {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_util::{StreamExt, stream};

    use super::*;

    #[tokio::test]
    async fn cancellation_ends_pending_stream_promptly() {
        let cancel = CancellationToken::new();
        let inner = stream::pending::<Result<ImagePullProgress, DockerError>>()
            .take_until(cancel.clone().cancelled_owned());
        let mut stream = ImagePullStream {
            inner: Box::pin(inner),
            cancel,
        };
        stream.cancel();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), stream.next())
                .await
                .expect("cancelled stream must wake")
                .is_none()
        );
    }

    #[test]
    fn drop_cancels_token() {
        let cancel = CancellationToken::new();
        let stream = ImagePullStream {
            inner: Box::pin(stream::empty()),
            cancel: cancel.clone(),
        };
        drop(stream);
        assert!(cancel.is_cancelled());
    }
}
