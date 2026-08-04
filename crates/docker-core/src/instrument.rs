//! Timing instrumentation for Docker service calls.
//!
//! Every measured call logs duration, result count, and cache status so the
//! actual bottlenecks can be confirmed from tracing output instead of
//! guessed (fix1.md section 29). Sensitive values are never logged: callers
//! pass only counts and safe labels.

use std::time::Instant;

use tracing::{debug, info};

/// Measure one Docker service call.
///
/// Usage:
/// ```ignore
/// let timer = Timer::start("docker.list_volumes");
/// let result = service.list_volumes(...).await;
/// timer.finish_ok(result_count, "live");
/// ```
pub struct Timer {
    name: &'static str,
    started: Instant,
}

impl Timer {
    pub fn start(name: &'static str) -> Self {
        Self {
            name,
            started: Instant::now(),
        }
    }

    pub fn finish_ok(&self, result_count: usize, source: &str) {
        info!(
            duration_ms = self.started.elapsed().as_millis() as u64,
            result_count,
            cache_source = source,
            "{}",
            self.name
        );
    }

    pub fn finish_err(&self, error: &str) {
        debug!(
            duration_ms = self.started.elapsed().as_millis() as u64,
            error, "{} failed", self.name
        );
    }

    /// A cache hit that needed no Docker call at all.
    pub fn finish_cache_hit(&self, result_count: usize, source: &str) {
        info!(
            duration_ms = self.started.elapsed().as_millis() as u64,
            result_count,
            cache_hit = true,
            cache_source = source,
            "{} (cache)",
            self.name
        );
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}
