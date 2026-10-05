//! Async client for the MPD protocol.
//!
//! Talks to MPD over tokio TCP sockets and parses responses with nom 8. Only the
//! subset of the protocol lyrica needs is implemented; commands are added as
//! features require them. See `docs/architecture.md` for the connection model.

mod client;
mod error;
mod idle;
mod parse;
mod search;
mod song;
mod status;

pub use client::{Client, Version};
pub use error::{Ack, Error, Result};
pub use idle::Subsystem;
pub use search::{Filter, QueueMatches, Tag};
pub use song::Song;
pub use status::{Single, State, Status};
