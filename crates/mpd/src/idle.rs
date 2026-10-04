/// `idle` subsystem.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Subsystem {
    Database,
    Update,
    StoredPlaylist,
    /// MPD's `playlist`.
    Queue,
    Player,
    Mixer,
    Output,
    Options,
    Partition,
    Sticker,
    Subscription,
    Message,
    Neighbor,
    Mount,
    /// Reported by a newer MPD.
    Other(String),
}

impl Subsystem {
    pub fn as_str(&self) -> &str {
        match self {
            Subsystem::Database => "database",
            Subsystem::Update => "update",
            Subsystem::StoredPlaylist => "stored_playlist",
            Subsystem::Queue => "playlist",
            Subsystem::Player => "player",
            Subsystem::Mixer => "mixer",
            Subsystem::Output => "output",
            Subsystem::Options => "options",
            Subsystem::Partition => "partition",
            Subsystem::Sticker => "sticker",
            Subsystem::Subscription => "subscription",
            Subsystem::Message => "message",
            Subsystem::Neighbor => "neighbor",
            Subsystem::Mount => "mount",
            Subsystem::Other(name) => name,
        }
    }

    pub(crate) fn parse(name: &str) -> Self {
        match name {
            "database" => Subsystem::Database,
            "update" => Subsystem::Update,
            "stored_playlist" => Subsystem::StoredPlaylist,
            "playlist" => Subsystem::Queue,
            "player" => Subsystem::Player,
            "mixer" => Subsystem::Mixer,
            "output" => Subsystem::Output,
            "options" => Subsystem::Options,
            "partition" => Subsystem::Partition,
            "sticker" => Subsystem::Sticker,
            "subscription" => Subsystem::Subscription,
            "message" => Subsystem::Message,
            "neighbor" => Subsystem::Neighbor,
            "mount" => Subsystem::Mount,
            other => Subsystem::Other(other.to_owned()),
        }
    }
}
