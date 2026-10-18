//! Moving live work between a person's provider accounts (M26).
//!
//! `policy` is what the person chose, `rank` decides where work goes, and the
//! relay and engine move it. See `docs/M26-ACCOUNT-ROTATION.md`.

pub mod policy;
pub mod rank;
pub mod relay;
pub mod engine;
