use std::{fmt, io};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub enum Error {
    /// Socket error or EOF. The client is unusable.
    Io(io::Error),
    /// MPD rejected the command. The client stays usable.
    Ack(Ack),
    /// Unparseable response. The client is unusable.
    Protocol(String),
    /// Argument contains a newline, which the protocol can't carry.
    InvalidArgument,
    /// An earlier command failed or was cancelled mid-response, so the
    /// connection is out of sync. Reconnect.
    Broken,
}

/// MPD error response: `ACK [code@index] {command} message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ack {
    /// `enum ack` in MPD's `src/protocol/Ack.hxx`.
    pub code: u32,
    /// Position of the failing command in a command list.
    pub index: u32,
    pub command: String,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "MPD connection: {e}"),
            Error::Ack(ack) => write!(f, "{ack}"),
            Error::Protocol(msg) => write!(f, "MPD protocol: {msg}"),
            Error::InvalidArgument => f.write_str("MPD argument contains a newline"),
            Error::Broken => f.write_str("MPD connection out of sync"),
        }
    }
}

impl fmt::Display for Ack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MPD error {} in {:?}: {}",
            self.code, self.command, self.message
        )
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}
