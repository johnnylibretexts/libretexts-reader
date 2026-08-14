//! Text-to-speech engines that run in the Rust process.
//!
//! Supertonic is the only speech engine. See ADR-0003 for why Kokoro, which
//! ran in the webview, was removed.

pub mod fish;
pub mod provider;
pub mod supertonic;
