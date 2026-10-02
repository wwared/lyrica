//! Bridge between WebSocket clients and MPD. Design: `docs/architecture.md`
//! → "MPD bridge".

use std::{
    convert::Infallible,
    sync::Arc,
    time::{Duration, Instant},
};

use lyrica_mpd::{Client, Error, Result, Song, State, Subsystem};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

const MIN_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(10);

pub struct Mpd {
    pub addr: String,
    pub password: Option<String>,
}

impl Mpd {
    async fn connect(&self) -> Result<Client> {
        let mut client = Client::connect(&*self.addr).await?;
        if let Some(password) = &self.password {
            client.password(password).await?;
        }
        Ok(client)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayState {
    Play,
    Pause,
    Stop,
    Offline,
}

impl From<State> for PlayState {
    fn from(state: State) -> Self {
        match state {
            State::Play => PlayState::Play,
            State::Pause => PlayState::Pause,
            State::Stop => PlayState::Stop,
        }
    }
}

/// Latest player state, published to every WebSocket client.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub state: PlayState,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub elapsed: Option<Duration>,
    pub duration: Option<Duration>,
    /// When `elapsed` was read, so clients that subscribe later still get
    /// the current position.
    pub at: Instant,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ServerMessage<'a> {
    State {
        state: PlayState,
        title: Option<&'a str>,
        artist: Option<&'a str>,
        elapsed: Option<f64>,
        duration: Option<f64>,
    },
}

impl Snapshot {
    pub fn offline() -> Self {
        Snapshot {
            state: PlayState::Offline,
            title: None,
            artist: None,
            elapsed: None,
            duration: None,
            at: Instant::now(),
        }
    }

    /// JSON `state` message as of `now`.
    pub fn message(&self, now: Instant) -> String {
        let mut elapsed = self.elapsed;
        if self.state == PlayState::Play {
            elapsed = elapsed.map(|e| e + now.saturating_duration_since(self.at));
            if let (Some(e), Some(d)) = (&mut elapsed, self.duration) {
                *e = (*e).min(d);
            }
        }
        let message = ServerMessage::State {
            state: self.state,
            title: self.title.as_deref(),
            artist: self.artist.as_deref(),
            elapsed: elapsed.map(|e| e.as_secs_f64()),
            duration: self.duration.map(|d| d.as_secs_f64()),
        };
        serde_json::to_string(&message).expect("state message serializes")
    }
}

/// Client → server message.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Command {
    Previous,
    Toggle,
    Next,
}

/// Keeps `snapshots` current, reconnecting with backoff while MPD is
/// unreachable.
pub async fn follow(mpd: Arc<Mpd>, snapshots: watch::Sender<Snapshot>) {
    let mut backoff = MIN_BACKOFF;
    loop {
        let error = match mpd.connect().await {
            Ok(client) => {
                tracing::info!("connected to MPD {} at {}", client.version(), mpd.addr);
                backoff = MIN_BACKOFF;
                let Err(error) = watch_player(client, &snapshots).await;
                error
            }
            Err(error) => error,
        };
        tracing::warn!("{error}; reconnecting to {} in {backoff:?}", mpd.addr);
        snapshots.send_if_modified(|snapshot| {
            let online = snapshot.state != PlayState::Offline;
            if online {
                *snapshot = Snapshot::offline();
            }
            online
        });
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

async fn watch_player(
    mut client: Client,
    snapshots: &watch::Sender<Snapshot>,
) -> Result<Infallible> {
    loop {
        snapshots.send_replace(fetch(&mut client).await?);
        client.idle(&[Subsystem::Player]).await?;
    }
}

async fn fetch(client: &mut Client) -> Result<Snapshot> {
    let status = client.status().await?;
    let at = Instant::now();
    let mut song = client.current_song().await?;
    Ok(Snapshot {
        state: status.state.into(),
        duration: status
            .duration
            .or_else(|| song.as_ref().and_then(|s| s.duration)),
        artist: song.as_mut().and_then(|s| s.artist.take()),
        title: song.map(title),
        elapsed: status.elapsed,
        at,
    })
}

fn title(song: Song) -> String {
    song.title
        .unwrap_or_else(|| match song.file.rsplit_once('/') {
            Some((_, name)) => name.to_owned(),
            None => song.file,
        })
}

/// Runs commands from all WebSocket clients, one at a time, on a dedicated
/// connection.
pub async fn execute_commands(
    mpd: Arc<Mpd>,
    mut commands: mpsc::Receiver<Command>,
    snapshots: watch::Receiver<Snapshot>,
) {
    let mut client = None;
    while let Some(command) = commands.recv().await {
        let state = snapshots.borrow().state;
        let Some(action) = Action::new(command, state) else {
            tracing::info!("MPD offline; dropping {command:?}");
            continue;
        };
        if let Err(error) = execute(&mpd, &mut client, action).await {
            tracing::warn!("{command:?}: {error}");
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Action {
    Play,
    Pause(bool),
    Previous,
    Next,
}

impl Action {
    /// Maps `toggle` from the current state, so it never sends a no-op:
    /// MPD emits no idle event for those, leaving clients without an update.
    fn new(command: Command, state: PlayState) -> Option<Self> {
        Some(match (command, state) {
            (_, PlayState::Offline) => return None,
            (Command::Previous, _) => Action::Previous,
            (Command::Next, _) => Action::Next,
            (Command::Toggle, PlayState::Play) => Action::Pause(true),
            (Command::Toggle, PlayState::Pause) => Action::Pause(false),
            (Command::Toggle, PlayState::Stop) => Action::Play,
        })
    }

    async fn run(self, client: &mut Client) -> Result<()> {
        match self {
            Action::Play => client.play().await,
            Action::Pause(pause) => client.pause(pause).await,
            Action::Previous => client.previous().await,
            Action::Next => client.next().await,
        }
    }
}

/// Runs `action`, reconnecting first if `client` is gone. A reused
/// connection may have been closed by MPD's `connection_timeout` without
/// running the command, so connection errors on it are retried once.
async fn execute(mpd: &Mpd, client: &mut Option<Client>, action: Action) -> Result<()> {
    let reused = client.is_some();
    let conn = match client {
        Some(conn) => conn,
        None => client.insert(mpd.connect().await?),
    };
    let mut result = action.run(conn).await;
    if reused && matches!(result, Err(Error::Io(_) | Error::Broken)) {
        tracing::debug!("command connection lost; reconnecting");
        *client = None;
        let conn = client.insert(mpd.connect().await?);
        result = action.run(conn).await;
    }
    if let Err(error) = &result
        && !matches!(error, Error::Ack(_))
    {
        *client = None;
    }
    result
}

#[cfg(test)]
mod tests {
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::TcpListener,
        sync::oneshot,
    };

    use super::*;

    fn json(snapshot: &Snapshot, now: Instant) -> serde_json::Value {
        serde_json::from_str(&snapshot.message(now)).unwrap()
    }

    #[test]
    fn late_subscribers_get_current_position() {
        let at = Instant::now();
        let mut snapshot = Snapshot {
            state: PlayState::Play,
            title: Some("t".into()),
            artist: Some("a".into()),
            elapsed: Some(Duration::from_secs(10)),
            duration: Some(Duration::from_secs(30)),
            at,
        };
        let later = at + Duration::from_secs(5);
        assert_eq!(
            json(&snapshot, later),
            serde_json::json!({
                "type": "state", "state": "play", "title": "t", "artist": "a",
                "elapsed": 15.0, "duration": 30.0,
            })
        );
        assert_eq!(
            json(&snapshot, at + Duration::from_secs(60))["elapsed"],
            30.0
        );
        snapshot.state = PlayState::Pause;
        assert_eq!(json(&snapshot, later)["elapsed"], 10.0);
    }

    #[test]
    fn title_falls_back_to_file_name() {
        let song = Song {
            file: "a/b/c.flac".into(),
            title: None,
            artist: None,
            album: None,
            duration: None,
            position: None,
            id: None,
        };
        assert_eq!(title(song), "c.flac");
    }

    async fn accept(listener: &TcpListener) -> BufReader<tokio::net::TcpStream> {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        stream.write_all(b"OK MPD 0.24.0\n").await.unwrap();
        stream
    }

    async fn expect(stream: &mut BufReader<tokio::net::TcpStream>, request: &str) {
        let mut line = String::new();
        stream.read_line(&mut line).await.unwrap();
        assert_eq!(line, request);
        stream.write_all(b"OK\n").await.unwrap();
    }

    #[tokio::test]
    async fn command_retried_after_mpd_closes_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mpd = Mpd {
            addr: listener.local_addr().unwrap().to_string(),
            password: Some("secret".into()),
        };
        let (closed_tx, closed) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut first = accept(&listener).await;
            expect(&mut first, "password \"secret\"\n").await;
            expect(&mut first, "next\n").await;
            drop(first);
            closed_tx.send(()).unwrap();
            let mut second = accept(&listener).await;
            expect(&mut second, "password \"secret\"\n").await;
            expect(&mut second, "pause \"1\"\n").await;
        });
        let mut client = None;
        execute(&mpd, &mut client, Action::Next).await.unwrap();
        closed.await.unwrap();
        execute(&mpd, &mut client, Action::Pause(true))
            .await
            .unwrap();
        server.await.unwrap();
    }
}
