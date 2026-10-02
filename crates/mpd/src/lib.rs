//! Async client for the MPD protocol.
//!
//! Talks to MPD over tokio TCP sockets and parses responses with nom 8. Only the
//! subset of the protocol lyrica needs is implemented; commands are added as
//! features require them. See `docs/architecture.md` for the connection model.
