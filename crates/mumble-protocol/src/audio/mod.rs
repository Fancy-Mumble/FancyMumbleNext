//! Fully decoupled audio processing pipeline.
//!
//! # Architecture
//!
//! ```text
//!         +-------------------- Outbound --------------------+
//!         |                                                  |
//! +-----------+   +-------------+   +----------+   +------------+
//! |  Capture  |-->| FilterChain |-->| Encoder  |-->|  Network   |
//! +-----------+   +-------------+   +----------+   +------------+
//!   mic / file     gate -> AGC ->       Opus            UDP
//!                  denoiser -> ...
//!
//!         +-------------------- Inbound ---------------------+
//!         |                                                  |
//! +------------+   +----------+   +-------------+   +-----------+
//! |  Network   |-->| Decoder  |-->| FilterChain |-->| Playback  |
//! +------------+   +----------+   +-------------+   +-----------+
//!    UDP              Opus          volume -> ...        speakers
//! ```
//!
//! Every box in the diagram is behind a trait so that implementations
//! can be swapped independently (SOLID / dependency inversion).
//!
//! # Sub-modules
//!
//! | Module     | Purpose |
//! |------------|---------|
//! | `sample`   | [`AudioFrame`], [`AudioFormat`], sample conversion helpers |
//! | `capture`  | [`AudioCapture`] trait + [`SilentCapture`] stub |
//! | `playback` | [`AudioPlayback`] trait + [`NullPlayback`] stub |
//! | `filter`   | [`AudioFilter`] trait, [`FilterChain`], concrete filters |
//! | `encoder`  | [`AudioEncoder`] trait + [`OpusEncoder`] stub |
//! | `decoder`  | [`AudioDecoder`] trait + [`OpusDecoder`] stub |
//! | `pipeline` | [`OutboundPipeline`] / [`InboundPipeline`] + builders |

pub mod capture;
pub mod decoder;
pub mod encoder;
pub mod filter;
pub mod pipeline;
pub mod playback;
pub mod sample;
