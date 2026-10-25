//! YouTube access for Katsanto.
//!
//! The rest of the application only talks to [`YoutubeSource`], so changes in
//! rustypipe (or YouTube) stay inside this crate.

pub mod channel;
mod client;
pub mod dash;
pub mod model;
mod player;
pub mod rss;
mod rustypipe_source;
pub mod solver;
mod source;

pub use client::{YoutubeClient, YoutubeConfig};
pub use player::USER_AGENT;
pub use source::{YoutubeSource, YtError, YtResult};
