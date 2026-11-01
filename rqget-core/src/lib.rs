//! Guarded foreground downloads with explicit destination ownership and progress observations.
//!
//! The runner embeds librqbit and owns shutdown, preflight validation, and the
//! destination writer lock. Callers provide engine options, an interruption future,
//! and a lightweight progress observer; terminal rendering belongs to the caller.

mod destination;
mod download;
mod request;

pub use download::{Event, Snapshot, run};
pub use request::{Request, Source};
