//! Optional music providers.
//!
//! M1 ships one provider, YouTube Music. It is deliberately concrete: a trait
//! with a single implementation would be untested indirection. The seam is this
//! module, so adding Audius or Internet Archive later does not touch the UI.

pub mod youtube;

pub use youtube::{BufferedStream, HelperVersion, ProviderError, YouTubeProvider};
