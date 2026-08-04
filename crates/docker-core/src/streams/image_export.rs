//! Cancellable streaming image export.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::Stream;
use tokio_util::{bytes::Bytes, sync::CancellationToken};

use crate::DockerError;

/// Byte stream for a Docker image TAR archive. Each item is forwarded as it is
/// received; the complete image is never buffered by docker-core.
pub struct ImageExportStream {
    pub(crate) inner: Pin<Box<dyn Stream<Item = Result<Bytes, DockerError>> + Send + 'static>>,
    pub(crate) cancel: CancellationToken,
}

impl ImageExportStream {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel.clone()
    }
}

impl Stream for ImageExportStream {
    type Item = Result<Bytes, DockerError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

impl Drop for ImageExportStream {
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
    async fn cancellation_ends_pending_byte_stream_promptly() {
        let cancel = CancellationToken::new();
        let inner = stream::pending::<Result<Bytes, DockerError>>()
            .take_until(cancel.clone().cancelled_owned());
        let mut stream = ImageExportStream {
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
        let stream = ImageExportStream {
            inner: Box::pin(stream::empty()),
            cancel: cancel.clone(),
        };
        drop(stream);
        assert!(cancel.is_cancelled());
    }
}
