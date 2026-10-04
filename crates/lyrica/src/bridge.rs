//! Bridge between WebSocket clients and MPD. Design: `docs/architecture.md`
//! → "MPD bridge".

use std::{
    convert::Infallible,
    ops::Range,
    sync::Arc,
    time::{Duration, Instant},
};

use lyrica_mpd::{Client, Error, Result, Single, Song, State, Subsystem};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

const MIN_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(10);
/// Most entries per window: ~0.8 ms of MPD time (`docs/architecture.md` →
/// "Queue measurements").
const MAX_WINDOW: u32 = 200;
/// Escaped in URL path segments: everything but RFC 3986 unreserved characters.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

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
    pub album: Option<String>,
    /// URL of the song's folder on the music mirror, ending in `/`.
    pub folder: Option<String>,
    pub elapsed: Option<Duration>,
    pub duration: Option<Duration>,
    /// 0–100; `None` when MPD can't tell.
    pub volume: Option<u8>,
    pub random: bool,
    pub repeat: bool,
    /// Also on for MPD's `oneshot`.
    pub single: bool,
    /// `None` while offline.
    pub queue: Option<Queue>,
    /// When `elapsed` was read, so clients that subscribe later still get
    /// the current position.
    pub at: Instant,
}

/// Queue state from `status`; entries travel in their own messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Queue {
    pub version: u32,
    pub length: u32,
    /// Current song's position and id; `None` when there is none.
    pub position: Option<u32>,
    pub id: Option<u32>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ServerMessage<'a> {
    State {
        state: PlayState,
        title: Option<&'a str>,
        artist: Option<&'a str>,
        album: Option<&'a str>,
        folder: Option<&'a str>,
        elapsed: Option<f64>,
        duration: Option<f64>,
        volume: Option<u8>,
        random: bool,
        repeat: bool,
        single: bool,
        queue: Option<Queue>,
    },
    Queue {
        version: u32,
        start: u32,
        songs: Option<Vec<Entry>>,
    },
}

/// Queue entry as shown in the list; its position is implied by the window.
#[derive(Debug, Serialize)]
struct Entry {
    id: Option<u32>,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    track: Option<u32>,
    duration: Option<f64>,
    folder: Option<String>,
}

impl Entry {
    fn new(mut song: Song, music_url: Option<&str>) -> Self {
        Entry {
            id: song.id,
            artist: artist(&mut song),
            album: album(&mut song),
            track: song.track,
            duration: song.duration.map(|d| d.as_secs_f64()),
            folder: music_url.and_then(|m| folder(m, &song.file)),
            title: title(song),
        }
    }
}

impl Snapshot {
    pub fn offline() -> Self {
        Snapshot {
            state: PlayState::Offline,
            title: None,
            artist: None,
            album: None,
            folder: None,
            elapsed: None,
            duration: None,
            volume: None,
            random: false,
            repeat: false,
            single: false,
            queue: None,
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
            album: self.album.as_deref(),
            folder: self.folder.as_deref(),
            elapsed: elapsed.map(|e| e.as_secs_f64()),
            duration: self.duration.map(|d| d.as_secs_f64()),
            volume: self.volume,
            random: self.random,
            repeat: self.repeat,
            single: self.single,
            queue: self.queue,
        };
        serde_json::to_string(&message).expect("state message serializes")
    }
}

/// Client → server message. `random`, `repeat` and `single` toggle the mode;
/// `volume` sets MPD's volume; `seek` jumps to `position` seconds in the
/// current song.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Command {
    Previous,
    Toggle,
    Next,
    Random,
    Repeat,
    Single,
    Volume {
        volume: u8,
    },
    Seek {
        #[serde(deserialize_with = "seconds")]
        position: Duration,
    },
}

fn seconds<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    Duration::try_from_secs_f64(f64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}

/// Client request for the queue entries at positions `start..end`, made with
/// queue `version`. Requests with `start` > `end` are invalid; `end` is
/// clamped to `start + MAX_WINDOW`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(try_from = "RawWindow")]
pub struct Window {
    version: u32,
    start: u32,
    end: u32,
}

#[derive(Deserialize)]
struct RawWindow {
    version: u32,
    start: u32,
    end: u32,
}

impl TryFrom<RawWindow> for Window {
    type Error = &'static str;

    fn try_from(raw: RawWindow) -> Result<Self, Self::Error> {
        if raw.start > raw.end {
            return Err("window starts after its end");
        }
        Ok(Window {
            version: raw.version,
            start: raw.start,
            end: raw.end.min(raw.start.saturating_add(MAX_WINDOW)),
        })
    }
}

impl Window {
    fn range(self) -> Range<u32> {
        self.start..self.end
    }

    /// JSON `queue` reply for a window that couldn't be served now.
    pub fn unserved(self) -> String {
        self.reply(None)
    }

    fn reply(self, songs: Option<Vec<Entry>>) -> String {
        let message = ServerMessage::Queue {
            version: self.version,
            start: self.start,
            songs,
        };
        serde_json::to_string(&message).expect("queue message serializes")
    }
}

/// A window and where its reply goes.
pub struct WindowRequest {
    pub window: Window,
    pub reply: mpsc::UnboundedSender<String>,
}

/// Keeps `snapshots` current, reconnecting with backoff while MPD is
/// unreachable. `music_url` is the HTTP mirror of MPD's music directory,
/// ending in `/`.
pub async fn follow(mpd: Arc<Mpd>, music_url: Option<String>, snapshots: watch::Sender<Snapshot>) {
    let mut backoff = MIN_BACKOFF;
    loop {
        let error = match mpd.connect().await {
            Ok(client) => {
                tracing::info!("connected to MPD {} at {}", client.version(), mpd.addr);
                backoff = MIN_BACKOFF;
                let Err(error) = watch_player(client, music_url.as_deref(), &snapshots).await;
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
    music_url: Option<&str>,
    snapshots: &watch::Sender<Snapshot>,
) -> Result<Infallible> {
    loop {
        snapshots.send_replace(fetch(&mut client, music_url).await?);
        client
            .idle(&[
                Subsystem::Player,
                Subsystem::Options,
                Subsystem::Mixer,
                Subsystem::Queue,
            ])
            .await?;
    }
}

async fn fetch(client: &mut Client, music_url: Option<&str>) -> Result<Snapshot> {
    let status = client.status().await?;
    let at = Instant::now();
    let mut song = client.current_song().await?;
    Ok(Snapshot {
        state: status.state.into(),
        duration: status
            .duration
            .or_else(|| song.as_ref().and_then(|s| s.duration)),
        artist: song.as_mut().and_then(artist),
        album: song.as_mut().and_then(album),
        folder: music_url
            .zip(song.as_ref())
            .and_then(|(m, s)| folder(m, &s.file)),
        title: song.map(title),
        elapsed: status.elapsed,
        volume: status.volume,
        random: status.random,
        repeat: status.repeat,
        single: status.single != Single::Off,
        queue: Some(Queue {
            version: status.queue_version,
            length: status.queue_length,
            position: status.song,
            id: status.song_id,
        }),
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

/// The first `Artist` tag, falling back to the first `AlbumArtist`.
fn artist(song: &mut Song) -> Option<String> {
    song.artist.take().or_else(|| song.album_artist.take())
}

/// The `Album` tag, falling back to the name of the file's folder; `None` for
/// streams and files at the top of the music directory.
fn album(song: &mut Song) -> Option<String> {
    song.album.take().or_else(|| {
        if song.file.contains("://") {
            return None;
        }
        let (dir, _) = song.file.rsplit_once('/')?;
        Some(dir.rsplit('/').next().unwrap_or(dir).to_owned())
    })
}

/// URL of the folder holding `file` under `music_url` (ending in `/`);
/// `None` for streams, which have no folder.
fn folder(music_url: &str, file: &str) -> Option<String> {
    if file.contains("://") {
        return None;
    }
    let mut url = music_url.to_owned();
    if let Some((dir, _)) = file.rsplit_once('/') {
        for segment in dir.split('/') {
            url.extend(utf8_percent_encode(segment, SEGMENT));
            url.push('/');
        }
    }
    Some(url)
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
        let action = Action::new(command, &snapshots.borrow());
        let Some(action) = action else {
            tracing::info!("MPD offline; dropping {command:?}");
            continue;
        };
        if let Err(error) = execute(&mpd, &mut client, async move |c| action.run(c).await).await {
            tracing::warn!("{command:?}: {error}");
        }
    }
}

/// Serves queue windows for all WebSocket clients, one at a time, on a
/// dedicated connection, so state pushes and commands never wait on them.
pub async fn serve_windows(
    mpd: Arc<Mpd>,
    music_url: Option<String>,
    mut requests: mpsc::Receiver<WindowRequest>,
    snapshots: watch::Receiver<Snapshot>,
) {
    let mut client = None;
    while let Some(WindowRequest { window, reply }) = requests.recv().await {
        if reply.is_closed() {
            continue;
        }
        let songs = if window.start == window.end {
            Some(Vec::new())
        } else if snapshots.borrow().state == PlayState::Offline {
            None
        } else {
            match execute(&mpd, &mut client, async move |c| {
                c.queue_range(window.range()).await
            })
            .await
            {
                Ok(songs) => Some(songs),
                // The queue changed under the client; its state push follows.
                Err(Error::Ack(ack)) => {
                    tracing::debug!("{window:?}: {ack}");
                    Some(Vec::new())
                }
                Err(error) => {
                    tracing::warn!("{window:?}: {error}");
                    None
                }
            }
        };
        let songs = songs.map(|songs| {
            let music_url = music_url.as_deref();
            songs
                .into_iter()
                .map(|s| Entry::new(s, music_url))
                .collect()
        });
        // The client may have disconnected meanwhile.
        let _ = reply.send(window.reply(songs));
    }
}

#[derive(Debug, Clone, Copy)]
enum Action {
    Play,
    Pause(bool),
    Previous,
    Next,
    Random(bool),
    Repeat(bool),
    Single(Single),
    Volume(u8),
    Seek(Duration),
}

impl Action {
    /// Maps toggles from the current state, so they never send a no-op: MPD
    /// emits no idle event for those, leaving clients without an update.
    fn new(command: Command, snapshot: &Snapshot) -> Option<Self> {
        Some(match (command, snapshot.state) {
            (_, PlayState::Offline) => return None,
            (Command::Previous, _) => Action::Previous,
            (Command::Next, _) => Action::Next,
            (Command::Toggle, PlayState::Play) => Action::Pause(true),
            (Command::Toggle, PlayState::Pause) => Action::Pause(false),
            (Command::Toggle, PlayState::Stop) => Action::Play,
            (Command::Random, _) => Action::Random(!snapshot.random),
            (Command::Repeat, _) => Action::Repeat(!snapshot.repeat),
            (Command::Single, _) if snapshot.single => Action::Single(Single::Off),
            (Command::Single, _) => Action::Single(Single::On),
            (Command::Volume { volume }, _) => Action::Volume(volume),
            (Command::Seek { position }, _) => Action::Seek(position),
        })
    }

    async fn run(self, client: &mut Client) -> Result<()> {
        match self {
            Action::Play => client.play().await,
            Action::Pause(pause) => client.pause(pause).await,
            Action::Previous => client.previous().await,
            Action::Next => client.next().await,
            Action::Random(on) => client.random(on).await,
            Action::Repeat(on) => client.repeat(on).await,
            Action::Single(mode) => client.single(mode).await,
            Action::Volume(volume) => client.set_volume(volume).await,
            Action::Seek(position) => client.seek_current(position).await,
        }
    }
}

/// Runs `run`, reconnecting first if `client` is gone. A reused connection
/// may have been closed by MPD's `connection_timeout` without running the
/// command, so connection errors on it are retried once.
async fn execute<T>(
    mpd: &Mpd,
    client: &mut Option<Client>,
    mut run: impl AsyncFnMut(&mut Client) -> Result<T>,
) -> Result<T> {
    let reused = client.is_some();
    let conn = match client {
        Some(conn) => conn,
        None => client.insert(mpd.connect().await?),
    };
    let mut result = run(conn).await;
    if reused && matches!(result, Err(Error::Io(_) | Error::Broken)) {
        tracing::debug!("MPD connection lost; reconnecting");
        *client = None;
        let conn = client.insert(mpd.connect().await?);
        result = run(conn).await;
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
            album: Some("al".into()),
            folder: Some("https://m/a/".into()),
            elapsed: Some(Duration::from_secs(10)),
            duration: Some(Duration::from_secs(30)),
            volume: Some(70),
            random: true,
            repeat: false,
            single: true,
            queue: Some(Queue {
                version: 7,
                length: 300,
                position: Some(12),
                id: Some(40),
            }),
            at,
        };
        let later = at + Duration::from_secs(5);
        assert_eq!(
            json(&snapshot, later),
            serde_json::json!({
                "type": "state", "state": "play", "title": "t", "artist": "a", "album": "al",
                "folder": "https://m/a/", "elapsed": 15.0, "duration": 30.0, "volume": 70,
                "random": true, "repeat": false, "single": true,
                "queue": {"version": 7, "length": 300, "position": 12, "id": 40},
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
    fn mode_toggles_invert_the_current_mode() {
        let mut snapshot = Snapshot {
            state: PlayState::Pause,
            random: true,
            ..Snapshot::offline()
        };
        let action = |command, snapshot: &Snapshot| format!("{:?}", Action::new(command, snapshot));
        assert_eq!(action(Command::Random, &snapshot), "Some(Random(false))");
        assert_eq!(action(Command::Repeat, &snapshot), "Some(Repeat(true))");
        assert_eq!(action(Command::Single, &snapshot), "Some(Single(On))");
        snapshot.single = true;
        assert_eq!(action(Command::Single, &snapshot), "Some(Single(Off))");
        snapshot.state = PlayState::Offline;
        assert_eq!(action(Command::Random, &snapshot), "None");
    }

    #[test]
    fn seek_takes_non_negative_seconds() {
        let parse = |json| serde_json::from_str::<Command>(json).map(|c| format!("{c:?}"));
        assert_eq!(
            parse(r#"{"type":"seek","position":83.25}"#).unwrap(),
            "Seek { position: 83.25s }"
        );
        assert!(parse(r#"{"type":"seek","position":-1}"#).is_err());
        assert!(parse(r#"{"type":"seek","position":1e300}"#).is_err());
    }

    #[test]
    fn folder_url_escapes_each_segment() {
        let m = "https://m/";
        assert_eq!(
            folder(m, "Röyksopp/A, B #1 [x?]/50% (ok)/01 t.flac").unwrap(),
            "https://m/R%C3%B6yksopp/A%2C%20B%20%231%20%5Bx%3F%5D/50%25%20%28ok%29/"
        );
        assert_eq!(folder(m, "a-b_c.d~/t.mp3").unwrap(), "https://m/a-b_c.d~/");
        assert_eq!(folder(m, "top.mp3").unwrap(), m);
        assert_eq!(folder(m, "http://radio/stream"), None);
    }

    fn song(file: &str) -> Song {
        Song {
            file: file.into(),
            title: None,
            artist: None,
            album_artist: None,
            album: None,
            track: None,
            duration: None,
            position: None,
            id: None,
        }
    }

    #[test]
    fn title_falls_back_to_file_name() {
        assert_eq!(title(song("a/b/c.flac")), "c.flac");
    }

    #[test]
    fn album_falls_back_to_folder_name() {
        let mut tagged = song("a/b/c.flac");
        tagged.album = Some("al".into());
        assert_eq!(album(&mut tagged).as_deref(), Some("al"));
        assert_eq!(album(&mut song("a/b/c.flac")).as_deref(), Some("b"));
        assert_eq!(album(&mut song("b/c.flac")).as_deref(), Some("b"));
        assert_eq!(album(&mut song("c.flac")), None);
        assert_eq!(album(&mut song("http://radio/a/stream")), None);
    }

    async fn accept(listener: &TcpListener) -> BufReader<tokio::net::TcpStream> {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        stream.write_all(b"OK MPD 0.24.0\n").await.unwrap();
        stream
    }

    async fn expect(stream: &mut BufReader<tokio::net::TcpStream>, request: &str) {
        reply(stream, request, "").await;
    }

    /// Reads `request`, then sends `response` and `OK`.
    async fn reply(stream: &mut BufReader<tokio::net::TcpStream>, request: &str, response: &str) {
        let mut line = String::new();
        stream.read_line(&mut line).await.unwrap();
        assert_eq!(line, request);
        stream
            .write_all(format!("{response}OK\n").as_bytes())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn queue_change_pushes_state() {
        const IDLE: &str = "idle \"player\" \"options\" \"mixer\" \"playlist\"\n";
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mpd = Mpd {
            addr: listener.local_addr().unwrap().to_string(),
            password: None,
        };
        let (changed_tx, changed) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut s = accept(&listener).await;
            let status = "state: play\nplaylist: 2\nplaylistlength: 3\nsong: 1\nsongid: 5\n";
            reply(&mut s, "status\n", status).await;
            reply(&mut s, "currentsong\n", "file: a.flac\n").await;
            changed.await.unwrap();
            reply(&mut s, IDLE, "changed: playlist\n").await;
            let status = "state: stop\nplaylist: 3\nplaylistlength: 0\n";
            reply(&mut s, "status\n", status).await;
            reply(&mut s, "currentsong\n", "").await;
            let mut line = String::new();
            s.read_line(&mut line).await.unwrap();
            assert_eq!(line, IDLE);
        });
        let client = mpd.connect().await.unwrap();
        let (tx, mut rx) = watch::channel(Snapshot::offline());
        let watcher = tokio::spawn(async move { watch_player(client, None, &tx).await });
        rx.changed().await.unwrap();
        let queue = |rx: &mut watch::Receiver<Snapshot>| rx.borrow_and_update().queue;
        let playing = Queue {
            version: 2,
            length: 3,
            position: Some(1),
            id: Some(5),
        };
        assert_eq!(queue(&mut rx), Some(playing));
        changed_tx.send(()).unwrap();
        rx.changed().await.unwrap();
        let cleared = Queue {
            version: 3,
            length: 0,
            position: None,
            id: None,
        };
        assert_eq!(queue(&mut rx), Some(cleared));
        server.await.unwrap();
        watcher.abort();
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
        execute(&mpd, &mut client, async |c| c.next().await)
            .await
            .unwrap();
        closed.await.unwrap();
        execute(&mpd, &mut client, async |c| c.pause(true).await)
            .await
            .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn windows_are_capped_and_failures_reported() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mpd = Arc::new(Mpd {
            addr: listener.local_addr().unwrap().to_string(),
            password: None,
        });
        let server = tokio::spawn(async move {
            let mut s = accept(&listener).await;
            let songs = "file: a/b/c.flac\nTitle: t\nArtist: ar\nAlbum: al\nTrack: 3\n\
                         duration: 61.5\nPos: 0\nId: 9\n\
                         file: d/e.flac\nAlbumArtist: aa\nPos: 1\nId: 4\n";
            reply(&mut s, "playlistinfo \"0:2\"\n", songs).await;
            let mut line = String::new();
            s.read_line(&mut line).await.unwrap();
            assert_eq!(line, "playlistinfo \"7:207\"\n");
            s.write_all(b"ACK [2@0] {playlistinfo} Bad song index\n")
                .await
                .unwrap();
        });
        let playing = Snapshot {
            state: PlayState::Play,
            ..Snapshot::offline()
        };
        let (_snapshot_tx, snapshots) = watch::channel(playing);
        let (requests, request_rx) = mpsc::channel(4);
        let music_url = Some("https://m/".into());
        tokio::spawn(serve_windows(mpd, music_url, request_rx, snapshots));
        let (reply_tx, mut replies) = mpsc::unbounded_channel();
        let mut window = async |start: u32, end: u32| {
            let window = serde_json::json!({"version": 5, "start": start, "end": end});
            let request = WindowRequest {
                window: serde_json::from_value(window).unwrap(),
                reply: reply_tx.clone(),
            };
            requests.send(request).await.unwrap();
            serde_json::from_str::<serde_json::Value>(&replies.recv().await.unwrap()).unwrap()
        };
        assert_eq!(
            window(0, 2).await,
            serde_json::json!({"type": "queue", "version": 5, "start": 0, "songs": [
                {"id": 9, "title": "t", "artist": "ar", "album": "al", "track": 3,
                 "duration": 61.5, "folder": "https://m/a/b/"},
                {"id": 4, "title": "e.flac", "artist": "aa", "album": "d", "track": null,
                 "duration": null, "folder": "https://m/d/"},
            ]})
        );
        // Empty: answered without asking MPD, whose next line must be 7:207.
        assert_eq!(window(5, 5).await["songs"], serde_json::json!([]));
        // MPD rejects the range: nothing there.
        assert_eq!(window(7, 100_000).await["songs"], serde_json::json!([]));
        server.await.unwrap();
        // MPD gone after the retry: not served.
        let unserved =
            serde_json::json!({"type": "queue", "version": 5, "start": 0, "songs": null});
        assert_eq!(window(0, 2).await, unserved);
    }
}
