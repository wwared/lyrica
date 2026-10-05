//! Bridge between WebSocket clients and MPD. Design: `docs/architecture.md`
//! → "MPD bridge".

use std::{
    collections::VecDeque,
    convert::Infallible,
    sync::Arc,
    time::{Duration, Instant},
};

use lyrica_mpd::{Client, Error, Filter, Result, Single, Song, State, Subsystem};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use tokio::sync::{
    mpsc::{self, error::TryRecvError},
    watch,
};

use crate::search::{self, Search};

const MIN_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(10);
/// Most entries per window: ~0.8 ms of MPD time (`docs/architecture.md` →
/// "Queue measurements").
const MAX_WINDOW: u32 = 200;
/// Entries read per step of `group_start`'s walk back through the queue.
const WALK_WINDOW: u32 = 100;
/// Pause between search steps: a scan holds up every MPD client, and without
/// it the next scan would go before the `status` fetch a state change set
/// off meanwhile (`docs/architecture.md` → "Search" → "Server").
const STEP_GAP: Duration = Duration::from_millis(2);
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
    Playlists {
        names: Option<&'a [String]>,
    },
    Loaded {
        name: &'a str,
        error: Option<&'a str>,
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
    /// Whether it starts an album group; set by `entries`.
    start: bool,
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
            start: true,
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
/// current song; `playid` plays the queue entry with that id; `albumstart`
/// plays the current song's album group in order from its first entry.
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
    PlayId {
        id: u32,
    },
    AlbumStart,
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
    fn reply(self, songs: Option<Vec<Entry>>) -> String {
        let message = ServerMessage::Queue {
            version: self.version,
            start: self.start,
            songs,
        };
        serde_json::to_string(&message).expect("queue message serializes")
    }
}

/// Read served on the queue connection.
#[derive(Debug)]
pub enum Read {
    Window(Window),
    Playlists,
    /// Starts a search, replacing the connection's running one.
    Search {
        id: u32,
        filter: Filter,
        from: Option<u32>,
    },
    /// Fetches the chunk of matches `start..start + 500`.
    Matches {
        id: u32,
        filter: Filter,
        start: u32,
    },
    /// Drops the connection's search.
    SearchEnd,
}

impl Read {
    /// JSON reply for a read that couldn't be served now, if it gets one.
    pub fn unserved(&self) -> Option<String> {
        match self {
            Read::Window(window) => Some(window.reply(None)),
            Read::Playlists => Some(playlists(None)),
            Read::Search { id, .. } => Some(search::failed(*id, "Busy")),
            Read::Matches { id, start, .. } => Some(search::unserved(*id, *start)),
            Read::SearchEnd => None,
        }
    }
}

/// A read and where its reply goes.
pub struct ReadRequest {
    pub read: Read,
    pub reply: mpsc::UnboundedSender<String>,
}

/// Work for the command connection.
#[derive(Debug)]
pub enum Job {
    Command(Command),
    /// Replaces the queue with stored playlist `name`; `reply` gets the
    /// `loaded` message.
    Load {
        name: String,
        reply: mpsc::UnboundedSender<String>,
    },
}

fn playlists(names: Option<&[String]>) -> String {
    serde_json::to_string(&ServerMessage::Playlists { names })
        .expect("playlists message serializes")
}

/// JSON `loaded` reply: `error` is `None` once the queue holds `name`.
pub fn loaded(name: &str, error: Option<&str>) -> String {
    serde_json::to_string(&ServerMessage::Loaded { name, error })
        .expect("loaded message serializes")
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

/// Runs commands and loads from all WebSocket clients, one at a time, on a
/// dedicated connection.
pub async fn execute_commands(
    mpd: Arc<Mpd>,
    music_url: Option<String>,
    mut jobs: mpsc::Receiver<Job>,
    snapshots: watch::Receiver<Snapshot>,
) {
    // Shared, not borrowed, by each command's closure: a borrow fails rustc's
    // higher-ranked `Send` check on the spawned task.
    let music_url: Option<Arc<str>> = music_url.map(Into::into);
    let mut client = None;
    while let Some(job) = jobs.recv().await {
        match job {
            Job::Command(command) => {
                let action = Action::new(command, &snapshots.borrow());
                let Some(action) = action else {
                    tracing::info!("MPD offline; dropping {command:?}");
                    continue;
                };
                let music_url = music_url.clone();
                if let Err(error) = execute(&mpd, &mut client, async move |c| {
                    action.run(c, music_url.as_deref()).await
                })
                .await
                {
                    tracing::warn!("{command:?}: {error}");
                }
            }
            Job::Load { name, reply } => {
                let error = load(&mpd, &mut client, &name, &snapshots).await;
                if let Some(error) = &error {
                    tracing::warn!("loading playlist {name:?}: {error}");
                }
                // The client may have disconnected meanwhile.
                let _ = reply.send(loaded(&name, error.as_deref()));
            }
        }
    }
}

/// Replaces the queue with playlist `name`; the error to show on failure.
async fn load(
    mpd: &Mpd,
    client: &mut Option<Client>,
    name: &str,
    snapshots: &watch::Receiver<Snapshot>,
) -> Option<String> {
    if snapshots.borrow().state == PlayState::Offline {
        return Some("MPD offline".into());
    }
    // Owned: a borrowed name fails rustc's higher-ranked `Send` check on the
    // spawned task.
    let name = name.to_owned();
    let result = execute(mpd, client, async move |c| load_listed(c, &name).await).await;
    match result {
        Ok(true) => None,
        Ok(false) => Some("No such playlist".into()),
        Err(Error::Ack(ack)) => Some(ack.message),
        Err(error) => Some(error.to_string()),
    }
}

/// Replaces the queue with playlist `name` if MPD lists it; whether it did.
async fn load_listed(client: &mut Client, name: &str) -> Result<bool> {
    // `load` failing after `clear` leaves the queue empty, so a playlist
    // removed since the listing must not get that far.
    if !client.playlists().await?.iter().any(|n| n == name) {
        return Ok(false);
    }
    client.replace_queue(name).await?;
    Ok(true)
}

/// Serves queue windows, playlist listings and searches for all WebSocket
/// clients, one MPD command at a time, on a dedicated connection, so state
/// pushes and commands never wait on them. Waiting requests go before each
/// search step; searches of several connections take turns.
pub async fn serve_reads(
    mpd: Arc<Mpd>,
    music_url: Option<String>,
    mut requests: mpsc::Receiver<ReadRequest>,
    snapshots: watch::Receiver<Snapshot>,
) {
    let mut client = None;
    let mut searches: VecDeque<Search> = VecDeque::new();
    let mut stepped = false;
    loop {
        let request = match searches.pop_front() {
            None => requests.recv().await,
            Some(mut search) => match requests.try_recv() {
                Ok(request) => {
                    searches.push_front(search);
                    Some(request)
                }
                Err(TryRecvError::Empty) if stepped => {
                    searches.push_front(search);
                    stepped = false;
                    tokio::time::sleep(STEP_GAP).await;
                    continue;
                }
                Err(TryRecvError::Empty) => {
                    let offline = snapshots.borrow().state == PlayState::Offline;
                    stepped = !search.reply.is_closed();
                    if stepped && search.step(&mpd, &mut client, offline).await {
                        searches.push_back(search);
                    }
                    continue;
                }
                Err(TryRecvError::Disconnected) => None,
            },
        };
        let Some(ReadRequest { read, reply }) = request else {
            return;
        };
        if reply.is_closed() {
            continue;
        }
        let offline = snapshots.borrow().state == PlayState::Offline;
        let message = match read {
            Read::Window(window) => {
                let songs = if window.start == window.end {
                    Some(Vec::new())
                } else if offline {
                    None
                } else {
                    serve_window(&mpd, &mut client, music_url.as_deref(), window).await
                };
                window.reply(songs)
            }
            Read::Playlists if offline => playlists(None),
            Read::Playlists => {
                match execute(&mpd, &mut client, async |c| c.playlists().await).await {
                    Ok(mut names) => {
                        names.sort_unstable_by(|a, b| {
                            lowercase(a).cmp(lowercase(b)).then_with(|| a.cmp(b))
                        });
                        playlists(Some(&names))
                    }
                    Err(error) => {
                        tracing::warn!("listing playlists: {error}");
                        playlists(None)
                    }
                }
            }
            Read::Search { id, filter, from } => {
                searches.retain(|s| !s.reply.same_channel(&reply));
                searches.push_back(Search::new(id, filter, from, reply));
                continue;
            }
            Read::SearchEnd => {
                searches.retain(|s| !s.reply.same_channel(&reply));
                continue;
            }
            Read::Matches { id, filter, start } => {
                search::chunk(&mpd, &mut client, offline, id, filter, start).await
            }
        };
        // The client may have disconnected meanwhile.
        let _ = reply.send(message);
    }
}

fn lowercase(s: &str) -> impl Iterator<Item = char> + '_ {
    s.chars().flat_map(char::to_lowercase)
}

async fn serve_window(
    mpd: &Mpd,
    client: &mut Option<Client>,
    music_url: Option<&str>,
    window: Window,
) -> Option<Vec<Entry>> {
    // The entry before the window tells whether its first one starts a group.
    let from = window.start.saturating_sub(1);
    match execute(mpd, client, async move |c| {
        c.queue_range(from..window.end).await
    })
    .await
    {
        Ok(songs) => Some(entries(songs, from < window.start, music_url)),
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
}

/// Converts consecutive queue `songs`, flagging group starts; with `leading`,
/// the first one only decides the next one's flag and is dropped.
fn entries(songs: Vec<Song>, leading: bool, music_url: Option<&str>) -> Vec<Entry> {
    let mut songs = songs.into_iter().map(|s| Entry::new(s, music_url));
    let before = if leading { songs.next() } else { None };
    let mut entries: Vec<Entry> = Vec::with_capacity(songs.len());
    for mut entry in songs {
        let previous = entries.last().or(before.as_ref());
        entry.start = previous.is_none_or(|p| !same_group(p, &entry));
        entries.push(entry);
    }
    entries
}

/// Whether `b`, right after `a` in the queue, is in `a`'s album group: the one
/// rule behind the list's separators and covers (`docs/architecture.md` →
/// "Album grouping").
fn same_group(a: &Entry, b: &Entry) -> bool {
    a.album.is_some() && a.album == b.album && a.folder == b.folder
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
    PlayId(u32),
    AlbumStart,
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
            (Command::PlayId { id }, _) => Action::PlayId(id),
            (Command::AlbumStart, _) => Action::AlbumStart,
        })
    }

    /// `music_url` as given to `serve_reads`, so groups match the list's.
    async fn run(self, client: &mut Client, music_url: Option<&str>) -> Result<()> {
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
            Action::PlayId(id) => client.play_id(id).await,
            Action::AlbumStart => album_start(client, music_url).await,
        }
    }
}

/// Turns random and single off and plays the current song's album group from
/// its first entry; a playing first entry carries on (`docs/architecture.md`
/// → "Queue actions").
async fn album_start(client: &mut Client, music_url: Option<&str>) -> Result<()> {
    let status = client.status().await?;
    let Some(position) = status.song else {
        return Ok(());
    };
    let first = group_start(client, music_url, position).await?;
    let carry_on = status.song_id == Some(first) && status.state == State::Play;
    client.play_in_order((!carry_on).then_some(first)).await
}

/// Id of the first entry in the album group of the entry at `position`.
async fn group_start(client: &mut Client, music_url: Option<&str>, position: u32) -> Result<u32> {
    let mut end = position + 1;
    // The earliest entry known to be in the group.
    let mut first: Option<Entry> = None;
    'walk: loop {
        let start = end.saturating_sub(WALK_WINDOW);
        let songs = client.queue_range(start..end).await?;
        for song in songs.into_iter().rev() {
            let entry = Entry::new(song, music_url);
            if first.as_ref().is_some_and(|f| !same_group(&entry, f)) {
                break 'walk;
            }
            first = Some(entry);
        }
        if start == 0 {
            break;
        }
        end = start;
    }
    first
        .and_then(|f| f.id)
        .ok_or_else(|| Error::Protocol("album group start without an Id".into()))
}

/// Runs `run`, reconnecting first if `client` is gone. A reused connection
/// may have been closed by MPD's `connection_timeout` without running the
/// command, so connection errors on it are retried once.
pub async fn execute<T>(
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
    fn playid_takes_an_entry_id() {
        let parse = |json| serde_json::from_str::<Command>(json).map(|c| format!("{c:?}"));
        assert_eq!(
            parse(r#"{"type":"playid","id":5}"#).unwrap(),
            "PlayId { id: 5 }"
        );
        assert!(parse(r#"{"type":"playid","id":-1}"#).is_err());
        assert!(parse(r#"{"type":"playid","id":1.5}"#).is_err());
        assert!(parse(r#"{"type":"playid"}"#).is_err());
        let offline = Snapshot::offline();
        assert!(Action::new(Command::PlayId { id: 5 }, &offline).is_none());
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

    #[test]
    fn group_starts_flagged_from_the_entry_before() {
        let tagged = |file: &str, album: &str| Song {
            album: Some(album.into()),
            ..song(file)
        };
        let flags = |songs: Vec<Song>, leading, music_url| -> Vec<bool> {
            entries(songs, leading, music_url)
                .iter()
                .map(|e| e.start)
                .collect()
        };
        let m = Some("https://m/");
        let album = || {
            vec![
                tagged("x/1.flac", "a"),
                tagged("x/2.flac", "a"),
                tagged("y/1.flac", "a"),
                tagged("y/2.flac", "b"),
                song("y/3.flac"),
            ]
        };
        // From position 0: same album and folder continue; another folder,
        // another album or the folder-name fallback start a group.
        assert_eq!(flags(album(), false, m), [true, false, true, true, true]);
        // Inside a group, the leading entry is dropped.
        assert_eq!(flags(album(), true, m), [false, true, true, true]);
        // At a group start.
        assert_eq!(flags(album()[1..].to_vec(), true, m), [true, true, true]);
        // Without `--music-url`, by album name alone.
        assert_eq!(
            flags(album(), false, None),
            [true, false, false, true, true]
        );
        // No album (top of the music directory, streams): a group each.
        let loose = vec![
            song("1.flac"),
            song("2.flac"),
            song("http://r/s"),
            song("http://r/s"),
        ];
        assert_eq!(flags(loose, true, m), [true, true, true]);
        // Only the leading entry (the window starts at the queue's end).
        assert!(entries(vec![song("x/1.flac")], true, m).is_empty());
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
            let same = "file: a/b/c.flac\nAlbum: al\nPos: 4\nId: 2\n\
                        file: a/b/d.flac\nAlbum: al\nPos: 5\nId: 3\n";
            reply(&mut s, "playlistinfo \"4:6\"\n", same).await;
            let mut line = String::new();
            s.read_line(&mut line).await.unwrap();
            assert_eq!(line, "playlistinfo \"6:207\"\n");
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
        tokio::spawn(serve_reads(mpd, music_url, request_rx, snapshots));
        let (reply_tx, mut replies) = mpsc::unbounded_channel();
        let mut window = async |start: u32, end: u32| {
            let window = serde_json::json!({"version": 5, "start": start, "end": end});
            let request = ReadRequest {
                read: Read::Window(serde_json::from_value(window).unwrap()),
                reply: reply_tx.clone(),
            };
            requests.send(request).await.unwrap();
            serde_json::from_str::<serde_json::Value>(&replies.recv().await.unwrap()).unwrap()
        };
        assert_eq!(
            window(0, 2).await,
            serde_json::json!({"type": "queue", "version": 5, "start": 0, "songs": [
                {"id": 9, "title": "t", "artist": "ar", "album": "al", "track": 3,
                 "duration": 61.5, "folder": "https://m/a/b/", "start": true},
                {"id": 4, "title": "e.flac", "artist": "aa", "album": "d", "track": null,
                 "duration": null, "folder": "https://m/d/", "start": true},
            ]})
        );
        // Read with the entry before it, which is left out.
        assert_eq!(
            window(5, 6).await,
            serde_json::json!({"type": "queue", "version": 5, "start": 5, "songs": [
                {"id": 3, "title": "d.flac", "artist": null, "album": "al", "track": null,
                 "duration": null, "folder": "https://m/a/b/", "start": false},
            ]})
        );
        // Empty: answered without asking MPD, whose next line must be 6:207.
        assert_eq!(window(5, 5).await["songs"], serde_json::json!([]));
        // MPD rejects the range: nothing there.
        assert_eq!(window(7, 100_000).await["songs"], serde_json::json!([]));
        server.await.unwrap();
        // MPD gone after the retry: not served.
        let unserved =
            serde_json::json!({"type": "queue", "version": 5, "start": 0, "songs": null});
        assert_eq!(window(0, 2).await, unserved);
    }

    #[tokio::test]
    async fn playlists_listed_sorted_or_null() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mpd = Arc::new(Mpd {
            addr: listener.local_addr().unwrap().to_string(),
            password: None,
        });
        let server = tokio::spawn(async move {
            let mut s = accept(&listener).await;
            let listing = "playlist: b\nLast-Modified: 2026-01-01T00:00:00Z\n\
                           playlist: C\nplaylist: a\nplaylist: B\n";
            reply(&mut s, "listplaylists\n", listing).await;
            let mut line = String::new();
            s.read_line(&mut line).await.unwrap();
            assert_eq!(line, "listplaylists\n");
            s.write_all(b"ACK [52@0] {listplaylists} Failed\n")
                .await
                .unwrap();
        });
        let (snapshot_tx, snapshots) = watch::channel(Snapshot::offline());
        let (requests, request_rx) = mpsc::channel(4);
        tokio::spawn(serve_reads(mpd, None, request_rx, snapshots));
        let (reply_tx, mut replies) = mpsc::unbounded_channel();
        let mut list = async || {
            let request = ReadRequest {
                read: Read::Playlists,
                reply: reply_tx.clone(),
            };
            requests.send(request).await.unwrap();
            serde_json::from_str::<serde_json::Value>(&replies.recv().await.unwrap()).unwrap()
        };
        let unserved = serde_json::json!({"type": "playlists", "names": null});
        // Offline: answered without asking MPD, which would list names.
        assert_eq!(list().await, unserved);
        snapshot_tx.send_replace(Snapshot {
            state: PlayState::Stop,
            ..Snapshot::offline()
        });
        assert_eq!(
            list().await,
            serde_json::json!({"type": "playlists", "names": ["a", "B", "b", "C"]})
        );
        assert_eq!(list().await, unserved);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn loads_check_the_name_then_replace_the_queue() {
        const LISTING: &str = "playlist: x\nplaylist: big\n";
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mpd = Arc::new(Mpd {
            addr: listener.local_addr().unwrap().to_string(),
            password: None,
        });
        let server = tokio::spawn(async move {
            let mut s = accept(&listener).await;
            let list = async |s: &mut BufReader<tokio::net::TcpStream>, name: &str| {
                let mut lines = String::new();
                for _ in 0..4 {
                    s.read_line(&mut lines).await.unwrap();
                }
                assert_eq!(
                    lines,
                    format!("command_list_begin\nclear\nload \"{name}\"\ncommand_list_end\n")
                );
            };
            reply(&mut s, "listplaylists\n", LISTING).await;
            list(&mut s, "x").await;
            s.write_all(b"OK\n").await.unwrap();
            // Not listed: the next line is the following load's listing.
            reply(&mut s, "listplaylists\n", LISTING).await;
            reply(&mut s, "listplaylists\n", LISTING).await;
            list(&mut s, "big").await;
            s.write_all(b"ACK [51@1] {load} Playlist is too large\n")
                .await
                .unwrap();
        });
        let (snapshot_tx, snapshots) = watch::channel(Snapshot::offline());
        let (jobs, job_rx) = mpsc::channel(4);
        tokio::spawn(execute_commands(mpd, None, job_rx, snapshots));
        let (reply_tx, mut replies) = mpsc::unbounded_channel();
        let mut load = async |name: &str| {
            let job = Job::Load {
                name: name.into(),
                reply: reply_tx.clone(),
            };
            jobs.send(job).await.unwrap();
            serde_json::from_str::<serde_json::Value>(&replies.recv().await.unwrap()).unwrap()
        };
        let loaded = |name: &str, error: Option<&str>| serde_json::json!({"type": "loaded", "name": name, "error": error});
        // Offline: answered without asking MPD, which would load it.
        assert_eq!(load("x").await, loaded("x", Some("MPD offline")));
        snapshot_tx.send_replace(Snapshot {
            state: PlayState::Play,
            ..Snapshot::offline()
        });
        assert_eq!(load("x").await, loaded("x", None));
        assert_eq!(load("gone").await, loaded("gone", Some("No such playlist")));
        assert_eq!(
            load("big").await,
            loaded("big", Some("Playlist is too large"))
        );
        server.await.unwrap();
    }

    /// `playlistinfo` reply for positions `range`: `tags(n)` gives the lines
    /// before `Pos`; the id is `n + 1000`.
    fn listing(range: std::ops::Range<u32>, tags: impl Fn(u32) -> String) -> String {
        range
            .map(|n| format!("{}Pos: {n}\nId: {}\n", tags(n), n + 1000))
            .collect()
    }

    /// The id `group_start` finds from `position` in a queue tagged by `tags`,
    /// checking it reads windows starting at `starts`, each ending where the
    /// one before starts, the first at `position + 1`.
    async fn walk(tags: fn(u32) -> String, position: u32, starts: &'static [u32]) -> u32 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut s = accept(&listener).await;
            let mut end = position + 1;
            for &start in starts {
                let request = format!("playlistinfo \"{start}:{end}\"\n");
                reply(&mut s, &request, &listing(start..end, tags)).await;
                end = start;
            }
        });
        let mut client = Client::connect(addr).await.unwrap();
        let first = group_start(&mut client, None, position).await.unwrap();
        drop(client);
        server.await.unwrap();
        first
    }

    #[tokio::test]
    async fn group_start_walks_back_a_window_at_a_time() {
        assert_eq!(walk(|n| format!("file: a/{n}.flac\n"), 5, &[0]).await, 1000);
        // At a window's first entry: only the window before tells.
        let split = |n| format!("file: {}/{n}.flac\n", if n < 51 { "a" } else { "b" });
        assert_eq!(walk(split, 150, &[51, 0]).await, 1051);
        let split = |n| format!("file: {}/{n}.flac\n", if n < 150 { "a" } else { "b" });
        assert_eq!(walk(split, 150, &[51]).await, 1150);
        let split = |n| format!("file: {}/{n}.flac\n", if n < 120 { "a" } else { "b" });
        assert_eq!(walk(split, 350, &[251, 151, 51]).await, 1120);
        // No album: a group of its own, even among others without one.
        assert_eq!(walk(|n| format!("file: {n}.flac\n"), 5, &[0]).await, 1005);
    }

    #[tokio::test]
    async fn album_start_plays_the_group_from_its_first_entry() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mpd = Arc::new(Mpd {
            addr: listener.local_addr().unwrap().to_string(),
            password: None,
        });
        // Album `al` in folders x (0–2) and y (3–): two groups with `--music-url`.
        let tags = |n| {
            format!(
                "file: {}/{n}.flac\nAlbum: al\n",
                if n < 3 { "x" } else { "y" }
            )
        };
        let server = tokio::spawn(async move {
            let mut s = accept(&listener).await;
            let status = |state: &str, song: Option<u32>| {
                let song = song
                    .map(|n| format!("song: {n}\nsongid: {}\n", n + 1000))
                    .unwrap_or_default();
                format!("state: {state}\n{song}playlist: 1\nplaylistlength: 10\n")
            };
            let list = async |s: &mut BufReader<tokio::net::TcpStream>, id: Option<u32>| {
                let playid = id
                    .map(|id| format!("playid \"{id}\"\n"))
                    .unwrap_or_default();
                let mut lines = String::new();
                for _ in 0..4 + usize::from(id.is_some()) {
                    s.read_line(&mut lines).await.unwrap();
                }
                assert_eq!(
                    lines,
                    format!(
                        "command_list_begin\nrandom \"0\"\nsingle \"0\"\n{playid}command_list_end\n"
                    )
                );
                s.write_all(b"OK\n").await.unwrap();
            };
            // No current song: the next line is the following status.
            reply(&mut s, "status\n", &status("stop", None)).await;
            // The playing first entry carries on; otherwise it plays from its start.
            let cases = [
                ("play", 3, None),
                ("pause", 3, Some(1003)),
                ("stop", 3, Some(1003)),
                ("play", 5, Some(1003)),
            ];
            for (state, song, id) in cases {
                reply(&mut s, "status\n", &status(state, Some(song))).await;
                let request = format!("playlistinfo \"0:{}\"\n", song + 1);
                reply(&mut s, &request, &listing(0..song + 1, tags)).await;
                list(&mut s, id).await;
            }
        });
        let playing = Snapshot {
            state: PlayState::Play,
            ..Snapshot::offline()
        };
        let (_snapshot_tx, snapshots) = watch::channel(playing);
        let (jobs, job_rx) = mpsc::channel(8);
        let music_url = Some("https://m/".into());
        tokio::spawn(execute_commands(mpd, music_url, job_rx, snapshots));
        for _ in 0..5 {
            jobs.send(Job::Command(Command::AlbumStart)).await.unwrap();
        }
        server.await.unwrap();
    }

    type Answer = oneshot::Sender<String>;

    /// Queue length in the search tests.
    const LENGTH: u32 = 10_000;

    /// The queue task against a fake MPD driven by the test: each request
    /// MPD gets (a search as `search WORD START:END`, others unquoted) comes
    /// with a sender for its raw response.
    struct Searching {
        requests: mpsc::Sender<ReadRequest>,
        mpd: mpsc::UnboundedReceiver<(String, Answer)>,
        snapshots: watch::Sender<Snapshot>,
    }

    impl Searching {
        async fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mpd = Arc::new(Mpd {
                addr: listener.local_addr().unwrap().to_string(),
                password: None,
            });
            let (seen, mpd_rx) = mpsc::unbounded_channel();
            tokio::spawn(async move {
                let mut s = accept(&listener).await;
                loop {
                    let mut line = String::new();
                    if s.read_line(&mut line).await.unwrap() == 0 {
                        return;
                    }
                    let request = if line == "command_list_begin\n" {
                        let mut list = String::new();
                        for _ in 0..3 {
                            s.read_line(&mut list).await.unwrap();
                        }
                        assert!(list.starts_with("status\nplaylistsearch "), "{list}");
                        assert!(list.ends_with("\ncommand_list_end\n"), "{list}");
                        let after = |s: &'static str| list.split(s).nth(1).unwrap();
                        let word = after("(*UTF)").split('\'').next().unwrap();
                        let window = after("\"window\" \"").split('"').next().unwrap();
                        format!("search {word} {window}")
                    } else {
                        line.trim_end().replace('"', "")
                    };
                    let (answer, answered) = oneshot::channel::<String>();
                    if seen.send((request, answer)).is_err() {
                        return;
                    }
                    let Ok(text) = answered.await else { return };
                    s.write_all(text.as_bytes()).await.unwrap();
                }
            });
            let playing = Snapshot {
                state: PlayState::Play,
                ..Snapshot::offline()
            };
            let (snapshots, snapshot_rx) = watch::channel(playing);
            let (requests, request_rx) = mpsc::channel(8);
            tokio::spawn(serve_reads(mpd, None, request_rx, snapshot_rx));
            Searching {
                requests,
                mpd: mpd_rx,
                snapshots,
            }
        }

        async fn send(&self, read: Read, reply: &mpsc::UnboundedSender<String>) {
            let request = ReadRequest {
                read,
                reply: reply.clone(),
            };
            self.requests.send(request).await.unwrap();
        }

        /// The next request MPD gets.
        async fn next(&mut self) -> (String, Answer) {
            tokio::time::timeout(Duration::from_secs(5), self.mpd.recv())
                .await
                .expect("MPD request in time")
                .unwrap()
        }

        /// Answers the next request, a search, from the matches at `positions`.
        async fn serve(&mut self, version: u32, positions: &[u32]) -> String {
            let (request, answer) = self.next().await;
            answer.send(found(version, positions, &request)).unwrap();
            request
        }
    }

    fn search(id: u32, word: &str, from: Option<u32>) -> Read {
        let filter = crate::query::compile(word).unwrap();
        Read::Search { id, filter, from }
    }

    fn window(start: u32, end: u32) -> Read {
        let window = serde_json::json!({"version": 1, "start": start, "end": end});
        Read::Window(serde_json::from_value(window).unwrap())
    }

    /// Matches `START..END` of `positions` for `search WORD START:END`.
    fn chunk_of<'a>(positions: &'a [u32], request: &str) -> &'a [u32] {
        let window = request.rsplit(' ').next().unwrap();
        let (start, end) = window.split_once(':').unwrap();
        let clamp = |n: &str| n.parse::<usize>().unwrap().min(positions.len());
        &positions[clamp(start)..clamp(end)]
    }

    /// MPD's response to a search command, at queue `version`.
    fn found(version: u32, positions: &[u32], request: &str) -> String {
        let songs: String = chunk_of(positions, request)
            .iter()
            .map(|p| format!("file: f\nPos: {p}\n"))
            .collect();
        format!("playlist: {version}\nplaylistlength: {LENGTH}\n{songs}OK\n")
    }

    async fn received(replies: &mut mpsc::UnboundedReceiver<String>) -> serde_json::Value {
        let text = tokio::time::timeout(Duration::from_secs(5), replies.recv())
            .await
            .expect("reply in time")
            .unwrap();
        serde_json::from_str(&text).unwrap()
    }

    /// Whether `reply` is a search's last.
    fn last(reply: &serde_json::Value) -> bool {
        reply["type"] != "matches"
            || !reply["first"].is_null()
            || (reply["start"] == 0 && reply["positions"] == serde_json::json!([]))
    }

    #[tokio::test]
    async fn search_sends_chunk_zero_then_the_one_with_the_first_match_after_from() {
        let mut t = Searching::start().await;
        let every5: Vec<u32> = (0..2000).map(|i| i * 5).collect();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut run = async |positions: &[u32], from| {
            t.send(search(7, "x", from), &tx).await;
            let mut replies: Vec<serde_json::Value> = Vec::new();
            while replies.last().is_none_or(|r| !last(r)) {
                tokio::select! {
                    _ = t.serve(1, positions) => {}
                    reply = received(&mut rx) => replies.push(reply),
                }
            }
            replies
        };
        let chunk =
            |start: usize| serde_json::json!(every5[start.min(2000)..(start + 500).min(2000)]);
        for (from, start, first) in [
            (None, 0, 0),
            (Some(1000), 0, 201),
            // "After" is strictly after; the last of chunk 0, then a gap.
            (Some(2495), 500, 500),
            (Some(2497), 500, 500),
            (Some(4000), 500, 801),
            (Some(9000), 1500, 1801),
            // None after: the first match, chunk 0 again.
            (Some(9995), 0, 0),
            (Some(LENGTH + 5), 0, 0),
        ] {
            let replies = run(&every5, from).await;
            assert_eq!(replies[0]["start"], 0, "chunk 0 first, from {from:?}");
            for reply in &replies[..replies.len() - 1] {
                assert!(reply["first"].is_null());
            }
            for reply in &replies {
                let start = reply["start"].as_u64().unwrap() as usize;
                assert_eq!(reply["positions"], chunk(start));
            }
            assert_eq!(
                replies.last().unwrap(),
                &serde_json::json!({"type": "matches", "id": 7, "version": 1, "start": start,
                                    "positions": chunk(start), "first": first}),
                "from {from:?}"
            );
        }
        // Fewer than a chunk, none after `from`: wraps within chunk 0.
        let few = [3, 8];
        let replies = run(&few, Some(8)).await;
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0]["first"], 0);
        // No matches.
        let replies = run(&[], Some(8)).await;
        assert_eq!(
            replies,
            [
                serde_json::json!({"type": "matches", "id": 7, "version": 1, "start": 0,
                                "positions": [], "first": null})
            ]
        );
    }

    #[tokio::test]
    async fn waiting_reads_go_before_each_search_step_and_searches_take_turns() {
        let mut t = Searching::start().await;
        let every5: Vec<u32> = (0..2000).map(|i| i * 5).collect();
        // Dense, then sparse: chunk 0's density overshoots, so "a" takes
        // several steps.
        let skewed: Vec<u32> = (0..500).chain((0..100).map(|i| 500 + i * 90)).collect();
        let (a, _a_rx) = mpsc::unbounded_channel();
        let (b, _b_rx) = mpsc::unbounded_channel();
        t.send(search(1, "a", Some(9000)), &a).await;
        let (request, answer) = t.next().await;
        assert_eq!(request, "search a 0:500");
        // Queued while MPD runs the step.
        t.send(window(0, 2), &b).await;
        t.send(search(2, "b", Some(9000)), &b).await;
        answer.send(found(1, &skewed, &request)).unwrap();
        let (request, answer) = t.next().await;
        assert_eq!(request, "playlistinfo 0:2");
        answer.send("OK\n".into()).unwrap();
        let mut words = Vec::new();
        for _ in 0..4 {
            let (request, answer) = t.next().await;
            let word = request.split(' ').nth(1).unwrap().to_owned();
            let positions = if word == "a" { &skewed } else { &every5 };
            answer.send(found(1, positions, &request)).unwrap();
            words.push(word);
        }
        assert_eq!(words, ["a", "b", "a", "b"]);
    }

    #[tokio::test]
    async fn searches_end_when_replaced_ended_closed_or_stale() {
        let mut t = Searching::start().await;
        let every5: Vec<u32> = (0..2000).map(|i| i * 5).collect();
        let (tx, mut rx) = mpsc::unbounded_channel();
        // Proves the search before it ran no further step.
        let idle = async |t: &mut Searching, tx| {
            t.send(window(0, 2), tx).await;
            let (request, answer) = t.next().await;
            assert_eq!(request, "playlistinfo 0:2");
            answer.send("OK\n".into()).unwrap();
        };

        t.send(search(1, "a", Some(9000)), &tx).await;
        let (request, answer) = t.next().await;
        t.send(search(2, "b", Some(9000)), &tx).await;
        answer.send(found(1, &every5, &request)).unwrap();
        assert_eq!(received(&mut rx).await["id"], 1);
        let (request, answer) = t.next().await;
        assert_eq!(request, "search b 0:500");
        t.send(Read::SearchEnd, &tx).await;
        answer.send(found(1, &every5, &request)).unwrap();
        assert_eq!(received(&mut rx).await["id"], 2);
        idle(&mut t, &tx).await;
        received(&mut rx).await;

        let (closing, closing_rx) = mpsc::unbounded_channel();
        t.send(search(3, "c", Some(9000)), &closing).await;
        let (request, answer) = t.next().await;
        drop(closing_rx);
        answer.send(found(1, &every5, &request)).unwrap();
        idle(&mut t, &tx).await;
        received(&mut rx).await;

        // A new queue version, or an `ACK` after chunk 0 (the queue shrank).
        let probe_acks = "ACK [2@1] {playlistsearch} Bad song index\n";
        for (id, version, response) in [(4, 2, None), (5, 1, Some(probe_acks))] {
            t.send(search(id, "d", Some(9000)), &tx).await;
            t.serve(1, &every5).await;
            assert_eq!(received(&mut rx).await["first"], serde_json::Value::Null);
            let (request, answer) = t.next().await;
            let response = response.map_or_else(|| found(version, &every5, &request), String::from);
            answer.send(response).unwrap();
            assert_eq!(
                received(&mut rx).await,
                serde_json::json!({"type": "searchstale", "id": id})
            );
            idle(&mut t, &tx).await;
            received(&mut rx).await;
        }
    }

    #[tokio::test]
    async fn search_failures_and_chunk_fetches() {
        let mut t = Searching::start().await;
        let every5: Vec<u32> = (0..2000).map(|i| i * 5).collect();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let fetch = |start| Read::Matches {
            id: 3,
            filter: crate::query::compile("x").unwrap(),
            start,
        };
        t.snapshots.send_modify(|s| s.state = PlayState::Offline);
        t.send(search(1, "x", None), &tx).await;
        assert_eq!(
            received(&mut rx).await,
            serde_json::json!({"type": "searchfailed", "id": 1, "error": "MPD offline"})
        );
        t.send(fetch(500), &tx).await;
        assert_eq!(
            received(&mut rx).await,
            serde_json::json!({"type": "matches", "id": 3, "version": null, "start": 500,
                               "positions": null, "first": null})
        );
        t.snapshots.send_modify(|s| s.state = PlayState::Play);

        t.send(search(2, "x", None), &tx).await;
        let (_, answer) = t.next().await;
        let invalid = "playlist: 1\nplaylistlength: 10\nACK [2@1] {playlistsearch} \
                       Error in regex at offset 1: missing closing parenthesis\n";
        answer.send(invalid.into()).unwrap();
        assert_eq!(
            received(&mut rx).await,
            serde_json::json!({"type": "searchfailed", "id": 2,
                               "error": "Error in regex at offset 1: missing closing parenthesis"})
        );

        t.send(fetch(1500), &tx).await;
        assert_eq!(t.serve(4, &every5).await, "search x 1500:2000");
        assert_eq!(
            received(&mut rx).await,
            serde_json::json!({"type": "matches", "id": 3, "version": 4, "start": 1500,
                               "positions": every5[1500..], "first": null})
        );
        t.send(fetch(20_000), &tx).await;
        let (_, answer) = t.next().await;
        answer
            .send("ACK [2@1] {playlistsearch} Bad song index\n".into())
            .unwrap();
        assert_eq!(received(&mut rx).await["positions"], serde_json::json!([]));
    }
}
