//! Audio filter interface and filter-chain builder.
//!
//! Every filter implements [`AudioFilter`] and processes frames in-place.
//! Filters are composed into a chain via [`FilterChain`], which runs
//! them sequentially. New filters are added by implementing the trait
//! in a separate file - no existing code needs to change.

pub mod automatic_gain;
pub mod denoiser;
pub mod noise_gate;
pub mod volume;

use crate::audio::sample::AudioFrame;
use crate::error::Result;

/// A single processing stage that transforms audio in-place.
///
/// Implementations should be lightweight and real-time safe
/// (no allocations, no blocking I/O during [`process`]).
pub trait AudioFilter: Send + 'static {
    /// Human-readable name for logging / UI display.
    fn name(&self) -> &str;

    /// Process a frame of audio **in-place**.
    ///
    /// The filter may modify `frame.data` and must leave
    /// `frame.format` and `frame.sequence` unchanged.
    fn process(&mut self, frame: &mut AudioFrame) -> Result<()>;

    /// Reset any internal state (e.g. between voice transmissions).
    fn reset(&mut self);

    /// Whether this filter is currently enabled.
    fn is_enabled(&self) -> bool;

    /// Enable or disable this filter at runtime.
    fn set_enabled(&mut self, enabled: bool);
}

/// An ordered chain of [`AudioFilter`]s executed sequentially.
///
/// Filters run in insertion order. Disabled filters are skipped
/// automatically.
pub struct FilterChain {
    filters: Vec<Box<dyn AudioFilter>>,
}

impl FilterChain {
    /// Create an empty filter chain.
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
        }
    }

    /// Append a filter to the end of the chain.
    pub fn push(&mut self, filter: Box<dyn AudioFilter>) {
        self.filters.push(filter);
    }

    /// Process a frame through every enabled filter in order.
    pub fn process(&mut self, frame: &mut AudioFrame) -> Result<()> {
        for filter in &mut self.filters {
            if filter.is_enabled() {
                filter.process(frame)?;
            }
        }
        Ok(())
    }

    /// Reset all filters in the chain.
    pub fn reset(&mut self) {
        for filter in &mut self.filters {
            filter.reset();
        }
    }

    /// Number of filters (enabled or not) in the chain.
    pub fn len(&self) -> usize {
        self.filters.len()
    }

    /// Whether the chain is empty.
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }
}

impl Default for FilterChain {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FilterChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilterChain")
            .field("filter_count", &self.filters.len())
            .finish_non_exhaustive()
    }
}
