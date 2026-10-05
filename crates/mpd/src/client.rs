use std::{
    fmt::{self, Write as _},
    io,
    ops::Range,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader, ReadBuf},
    net::{TcpStream, ToSocketAddrs},
};

use crate::{
    Error, Filter, QueueMatches, Result, Single, Song, Status, Subsystem,
    parse::{self, Line},
};

/// Cap on the capacity reserved up front for a response's songs or positions.
const MAX_RESERVED_SONGS: usize = 1000;

/// Protocol version from MPD's greeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// One MPD connection, running one command at a time.
///
/// Cancelling a command (dropping its future) leaves the connection out of
/// sync; later commands then fail with [`Error::Broken`].
pub struct Client {
    stream: BufReader<QuickAck>,
    version: Version,
    request: String,
    response: String,
    /// Separate from `response`: tokio's `read_line` re-validates the UTF-8
    /// of the whole buffer on every call, quadratic over a long response.
    line: String,
    broken: bool,
}

impl Client {
    pub async fn connect(addr: impl ToSocketAddrs) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        stream.set_nodelay(true)?;
        let mut stream = BufReader::new(QuickAck(stream));
        let mut greeting = String::new();
        read_line(&mut stream, &mut greeting).await?;
        let version = parse::greeting(&greeting)
            .ok_or_else(|| Error::Protocol(format!("unexpected greeting: {greeting:?}")))?;
        Ok(Client {
            stream,
            version,
            request: String::new(),
            response: String::new(),
            line: String::new(),
            broken: false,
        })
    }

    pub fn version(&self) -> Version {
        self.version
    }

    pub async fn password(&mut self, password: &str) -> Result<()> {
        self.run("password", &[password]).await.map(drop)
    }

    pub async fn status(&mut self) -> Result<Status> {
        Status::from_pairs(self.run("status", &[]).await?)
    }

    pub async fn current_song(&mut self) -> Result<Option<Song>> {
        Song::from_pairs(self.run("currentsong", &[]).await?)
    }

    /// Queue entries at positions `range`. MPD clamps `range.end` to the
    /// queue length; a start past the end is an `ACK`.
    pub async fn queue_range(&mut self, range: Range<u32>) -> Result<Vec<Song>> {
        let capacity = range.len().min(MAX_RESERVED_SONGS);
        let arg = format!("{}:{}", range.start, range.end);
        Song::list_from_pairs(self.run("playlistinfo", &[&arg]).await?, capacity)
    }

    /// Queue positions of matches `window` of `filter` (match indices, in
    /// queue order; MPD clamps the end), with `status`'s queue version and
    /// length, in one command list. MPD rejects a start past the queue's
    /// length and an end over 2³¹ − 1 (`ACK`).
    pub async fn search_queue(
        &mut self,
        filter: &Filter,
        window: Range<u32>,
    ) -> Result<QueueMatches> {
        let mut text = String::new();
        write!(text, "{filter}").map_err(|_| Error::InvalidArgument)?;
        let capacity = window.len().min(MAX_RESERVED_SONGS);
        let window = format!("{}:{}", window.start, window.end);
        let pairs = self
            .run_list(&[
                ("status", &[]),
                ("playlistsearch", &[&text, "window", &window]),
            ])
            .await?;
        QueueMatches::from_pairs(pairs, capacity)
    }

    /// Starts playback, resuming if paused.
    pub async fn play(&mut self) -> Result<()> {
        self.run("play", &[]).await.map(drop)
    }

    /// Plays the queue entry with id `id`.
    pub async fn play_id(&mut self, id: u32) -> Result<()> {
        self.run("playid", &[&id.to_string()]).await.map(drop)
    }

    pub async fn pause(&mut self, pause: bool) -> Result<()> {
        self.run("pause", &[if pause { "1" } else { "0" }])
            .await
            .map(drop)
    }

    pub async fn previous(&mut self) -> Result<()> {
        self.run("previous", &[]).await.map(drop)
    }

    pub async fn next(&mut self) -> Result<()> {
        self.run("next", &[]).await.map(drop)
    }

    pub async fn random(&mut self, on: bool) -> Result<()> {
        self.run("random", &[if on { "1" } else { "0" }])
            .await
            .map(drop)
    }

    pub async fn repeat(&mut self, on: bool) -> Result<()> {
        self.run("repeat", &[if on { "1" } else { "0" }])
            .await
            .map(drop)
    }

    pub async fn single(&mut self, mode: Single) -> Result<()> {
        self.run("single", &[mode.as_str()]).await.map(drop)
    }

    /// `volume` 0–100 (MPD rejects more).
    pub async fn set_volume(&mut self, volume: u8) -> Result<()> {
        self.run("setvol", &[&volume.to_string()]).await.map(drop)
    }

    /// Seeks to `position` in the current song. MPD rejects it while stopped.
    pub async fn seek_current(&mut self, position: Duration) -> Result<()> {
        let seconds = format!("{:.3}", position.as_secs_f64());
        self.run("seekcur", &[&seconds]).await.map(drop)
    }

    /// Stored playlist names, in MPD's order.
    pub async fn playlists(&mut self) -> Result<Vec<String>> {
        let names = self
            .run("listplaylists", &[])
            .await?
            .filter(|(key, _)| *key == "playlist")
            .map(|(_, name)| name.to_owned())
            .collect();
        Ok(names)
    }

    /// Replaces the queue with stored playlist `name` in one command list,
    /// so other clients see a single change. Not a transaction: if `load`
    /// fails (`ACK` index 1), the queue stays cleared.
    pub async fn replace_queue(&mut self, name: &str) -> Result<()> {
        self.run_list(&[("clear", &[]), ("load", &[name])])
            .await
            .map(drop)
    }

    /// Turns random and single off, then plays entry `id` (if given) from its
    /// start, in one command list. Not a transaction: if `playid` fails
    /// (`ACK` index 2), the modes stay off.
    pub async fn play_in_order(&mut self, id: Option<u32>) -> Result<()> {
        let random = ("random", &["0"][..]);
        let single = ("single", &["0"][..]);
        match id {
            None => self.run_list(&[random, single]).await.map(drop),
            Some(id) => self
                .run_list(&[random, single, ("playid", &[&id.to_string()])])
                .await
                .map(drop),
        }
    }

    /// Waits until one of `subsystems` (all if empty) changes. Blocks the
    /// connection meanwhile, so idle on a dedicated client.
    pub async fn idle(&mut self, subsystems: &[Subsystem]) -> Result<Vec<Subsystem>> {
        let args: Vec<&str> = subsystems.iter().map(Subsystem::as_str).collect();
        let changed = self
            .run("idle", &args)
            .await?
            .filter(|(key, _)| *key == "changed")
            .map(|(_, name)| Subsystem::parse(name))
            .collect();
        Ok(changed)
    }

    /// Sends a command and returns its response pairs.
    async fn run(
        &mut self,
        command: &str,
        args: &[&str],
    ) -> Result<impl Iterator<Item = (&str, &str)> + use<'_>> {
        self.request.clear();
        encode(&mut self.request, command, args)?;
        self.send().await
    }

    /// Sends `commands` as one command list: one response, `OK` or the first
    /// failing command's `ACK`.
    async fn run_list(
        &mut self,
        commands: &[(&str, &[&str])],
    ) -> Result<impl Iterator<Item = (&str, &str)> + use<'_>> {
        self.request.clear();
        self.request.push_str("command_list_begin\n");
        for (command, args) in commands {
            encode(&mut self.request, command, args)?;
        }
        self.request.push_str("command_list_end\n");
        self.send().await
    }

    /// Writes `request` and reads the response pairs.
    async fn send(&mut self) -> Result<impl Iterator<Item = (&str, &str)>> {
        if self.broken {
            return Err(Error::Broken);
        }
        self.response.clear();
        self.broken = true;
        self.stream
            .get_mut()
            .0
            .write_all(self.request.as_bytes())
            .await?;
        loop {
            self.line.clear();
            read_line(&mut self.stream, &mut self.line).await?;
            match parse::line(&self.line) {
                Some(Line::Pair(..)) => {
                    self.response.push_str(&self.line);
                    self.response.push('\n');
                }
                Some(Line::Ok) => break,
                Some(Line::Ack(ack)) => {
                    self.broken = false;
                    return Err(Error::Ack(ack));
                }
                None => {
                    return Err(Error::Protocol(format!("unexpected line: {:?}", self.line)));
                }
            }
        }
        self.broken = false;
        Ok(self.response.lines().filter_map(parse::pair))
    }
}

/// The connection's socket, re-arming `TCP_QUICKACK` before every read:
/// MPD's Nagle holds the last segment of a multi-segment response until the
/// previous one is ACKed, which Linux otherwise delays by 40 ms. Linux drops
/// the flag again on its own, hence every read.
struct QuickAck(TcpStream);

impl AsyncRead for QuickAck {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let stream = &mut self.get_mut().0;
        #[cfg(target_os = "linux")]
        socket2::SockRef::from(&*stream).set_tcp_quickack(true)?;
        Pin::new(stream).poll_read(cx, buf)
    }
}

/// Appends one line to `buf`, without its newline.
async fn read_line(stream: &mut BufReader<QuickAck>, buf: &mut String) -> Result<()> {
    stream.read_line(buf).await?;
    if buf.pop() != Some('\n') {
        return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
    }
    Ok(())
}

/// Appends one command line to `out`.
fn encode(out: &mut String, command: &str, args: &[&str]) -> Result<()> {
    out.push_str(command);
    for arg in args {
        if arg.contains('\n') {
            return Err(Error::InvalidArgument);
        }
        out.push_str(" \"");
        for c in arg.chars() {
            if matches!(c, '"' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
    }
    out.push('\n');
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, time::Duration};

    use tokio::{net::TcpListener, task::JoinHandle};

    use super::*;
    use crate::{Ack, State, Tag};

    /// Fake MPD: expects each request in order, sends the scripted response,
    /// then waits for the client to disconnect.
    async fn fake(script: &'static [(&'static str, &'static str)]) -> (SocketAddr, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            stream.write_all(b"OK MPD 0.24.0\n").await.unwrap();
            let mut line = String::new();
            for (request, response) in script {
                line.clear();
                stream.read_line(&mut line).await.unwrap();
                assert_eq!(line, *request);
                stream.write_all(response.as_bytes()).await.unwrap();
            }
            line.clear();
            assert_eq!(stream.read_line(&mut line).await.unwrap(), 0, "{line:?}");
        });
        (addr, server)
    }

    #[tokio::test]
    async fn commands() {
        let (addr, server) = fake(&[
            ("password \"a\\\"b\\\\c\"\n", "OK\n"),
            (
                "status\n",
                "volume: 100\nplaylist: 2\nplaylistlength: 10\nstate: pause\nsong: 3\nsongid: 4\nOK\n",
            ),
            ("currentsong\n", "OK\n"),
            ("pause \"0\"\n", "OK\n"),
            ("next\n", "OK\n"),
            ("random \"1\"\n", "OK\n"),
            ("repeat \"0\"\n", "OK\n"),
            ("single \"oneshot\"\n", "OK\n"),
            ("setvol \"55\"\n", "OK\n"),
            ("seekcur \"83.250\"\n", "OK\n"),
        ])
        .await;
        let mut client = Client::connect(addr).await.unwrap();
        assert_eq!(client.version().to_string(), "0.24.0");
        client.password("a\"b\\c").await.unwrap();
        let status = client.status().await.unwrap();
        assert_eq!(status.state, State::Pause);
        assert_eq!(status.song, Some(3));
        assert_eq!(client.current_song().await.unwrap(), None);
        client.pause(false).await.unwrap();
        client.next().await.unwrap();
        client.random(true).await.unwrap();
        client.repeat(false).await.unwrap();
        client.single(Single::Oneshot).await.unwrap();
        client.set_volume(55).await.unwrap();
        client
            .seek_current(Duration::from_millis(83_250))
            .await
            .unwrap();
        drop(client);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn queue_range_and_play_id() {
        // Larger than the read buffer, so it takes several reads.
        let window: String = (100..400)
            .map(|pos| {
                format!(
                    "file: dir/{pos}.flac\nTitle: {}\nPos: {pos}\nId: {}\n",
                    "x".repeat(100),
                    pos + 1
                )
            })
            .chain(["OK\n".into()])
            .collect();
        let script = vec![
            ("playlistinfo \"100:400\"\n", &*window.leak()),
            ("playlistinfo \"5:5\"\n", "OK\n"),
            (
                "playlistinfo \"9:10\"\n",
                "ACK [2@0] {playlistinfo} Bad song index\n",
            ),
            ("playid \"7\"\n", "OK\n"),
        ];
        let (addr, server) = fake(script.leak()).await;
        let mut client = Client::connect(addr).await.unwrap();
        let songs = client.queue_range(100..400).await.unwrap();
        assert_eq!(songs.len(), 300);
        assert_eq!(songs[0].file, "dir/100.flac");
        assert_eq!(songs[299].id, Some(400));
        assert!(client.queue_range(5..5).await.unwrap().is_empty());
        assert!(matches!(
            client.queue_range(9..10).await,
            Err(Error::Ack(_))
        ));
        client.play_id(7).await.unwrap();
        drop(client);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn ack_keeps_connection_usable() {
        let (addr, server) = fake(&[
            ("play\n", "ACK [2@0] {play} Bad song index\n"),
            ("next\n", "OK\n"),
        ])
        .await;
        let mut client = Client::connect(addr).await.unwrap();
        match client.play().await {
            Err(Error::Ack(ack)) => assert_eq!(
                ack,
                Ack {
                    code: 2,
                    index: 0,
                    command: "play".into(),
                    message: "Bad song index".into(),
                }
            ),
            other => panic!("{other:?}"),
        }
        client.next().await.unwrap();
        drop(client);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn idle_reports_changes() {
        let (addr, server) = fake(&[(
            "idle \"player\" \"mixer\"\n",
            "changed: player\nchanged: mixer\nOK\n",
        )])
        .await;
        let mut client = Client::connect(addr).await.unwrap();
        let changed = client
            .idle(&[Subsystem::Player, Subsystem::Mixer])
            .await
            .unwrap();
        assert_eq!(changed, [Subsystem::Player, Subsystem::Mixer]);
        drop(client);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn cancelled_command_breaks_client() {
        let (addr, server) = fake(&[("idle\n", "")]).await;
        let mut client = Client::connect(addr).await.unwrap();
        let idle = tokio::time::timeout(Duration::from_millis(50), client.idle(&[])).await;
        assert!(idle.is_err());
        assert!(matches!(client.next().await, Err(Error::Broken)));
        drop(client);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn playlists() {
        let (addr, server) = fake(&[
            (
                "listplaylists\n",
                "playlist: b\nLast-Modified: 2024-01-01T00:00:00Z\nplaylist: a: \"x\" \\\nLast-Modified: 2024-01-02T00:00:00Z\nOK\n",
            ),
            ("listplaylists\n", "OK\n"),
        ])
        .await;
        let mut client = Client::connect(addr).await.unwrap();
        assert_eq!(client.playlists().await.unwrap(), ["b", "a: \"x\" \\"]);
        assert!(client.playlists().await.unwrap().is_empty());
        drop(client);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn replace_queue() {
        let (addr, server) = fake(&[
            ("command_list_begin\n", ""),
            ("clear\n", ""),
            ("load \"a \\\"b\\\" \\\\c\"\n", ""),
            ("command_list_end\n", "OK\n"),
            ("command_list_begin\n", ""),
            ("clear\n", ""),
            ("load \"gone\"\n", ""),
            ("command_list_end\n", "ACK [50@1] {load} No such playlist\n"),
            ("next\n", "OK\n"),
        ])
        .await;
        let mut client = Client::connect(addr).await.unwrap();
        client.replace_queue("a \"b\" \\c").await.unwrap();
        match client.replace_queue("gone").await {
            Err(Error::Ack(ack)) => assert_eq!(
                ack,
                Ack {
                    code: 50,
                    index: 1,
                    command: "load".into(),
                    message: "No such playlist".into(),
                }
            ),
            other => panic!("{other:?}"),
        }
        client.next().await.unwrap();
        drop(client);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn play_in_order() {
        let (addr, server) = fake(&[
            ("command_list_begin\n", ""),
            ("random \"0\"\n", ""),
            ("single \"0\"\n", ""),
            ("command_list_end\n", "OK\n"),
            ("command_list_begin\n", ""),
            ("random \"0\"\n", ""),
            ("single \"0\"\n", ""),
            ("playid \"42\"\n", ""),
            ("command_list_end\n", "OK\n"),
            ("command_list_begin\n", ""),
            ("random \"0\"\n", ""),
            ("single \"0\"\n", ""),
            ("playid \"7\"\n", ""),
            ("command_list_end\n", "ACK [50@2] {playid} No such song\n"),
            ("next\n", "OK\n"),
        ])
        .await;
        let mut client = Client::connect(addr).await.unwrap();
        client.play_in_order(None).await.unwrap();
        client.play_in_order(Some(42)).await.unwrap();
        match client.play_in_order(Some(7)).await {
            Err(Error::Ack(ack)) => assert_eq!(
                ack,
                Ack {
                    code: 50,
                    index: 2,
                    command: "playid".into(),
                    message: "No such song".into(),
                }
            ),
            other => panic!("{other:?}"),
        }
        client.next().await.unwrap();
        drop(client);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn search_queue() {
        // Larger than the read buffer, so it takes several reads.
        let found: String = (0..300)
            .map(|i| {
                format!(
                    "file: dir/{i}.mp3\nTitle: {}\nduration: 1.5\nPos: {}\nId: 9\n",
                    "x".repeat(100),
                    i * 3
                )
            })
            .collect();
        let found = format!("playlist: 2\nplaylistlength: 1000\nduration: 1.5\n{found}OK\n");
        let script = vec![
            ("command_list_begin\n", ""),
            ("status\n", ""),
            (
                concat!(
                    r#"playlistsearch "((title =~ '(*UTF)it\\\\\\'s a') AND (file !~ 'rock\\\\/pop'))" "window" "0:500""#,
                    "\n"
                ),
                "",
            ),
            ("command_list_end\n", &*found.leak()),
            ("command_list_begin\n", ""),
            ("status\n", ""),
            (
                "playlistsearch \"(any =~ 'none')\" \"window\" \"500:1000\"\n",
                "",
            ),
            (
                "command_list_end\n",
                "volume: 50\nplaylist: 3\nplaylistlength: 10\nOK\n",
            ),
            ("command_list_begin\n", ""),
            ("status\n", ""),
            ("playlistsearch \"(title =~ '(')\" \"window\" \"0:1\"\n", ""),
            (
                "command_list_end\n",
                "playlist: 3\nplaylistlength: 10\n\
                 ACK [2@1] {playlistsearch} Error in regex at offset 1: missing closing parenthesis\n",
            ),
            ("next\n", "OK\n"),
        ];
        let (addr, server) = fake(script.leak()).await;
        let mut client = Client::connect(addr).await.unwrap();
        let regex = |tag, pattern: &str, negated| Filter::Regex {
            tag,
            pattern: pattern.into(),
            negated,
        };
        let filter = Filter::And(vec![
            regex(Tag::Title, r"(*UTF)it\'s a", false),
            regex(Tag::File, r"rock\/pop", true),
        ]);
        let matches = client.search_queue(&filter, 0..500).await.unwrap();
        assert_eq!(matches.queue_version, 2);
        assert_eq!(matches.queue_length, 1000);
        assert_eq!(matches.positions.len(), 300);
        assert_eq!(matches.positions[299], 897);
        let none = client
            .search_queue(&regex(Tag::Any, "none", false), 500..1000)
            .await
            .unwrap();
        assert_eq!((none.queue_version, none.queue_length), (3, 10));
        assert!(none.positions.is_empty());
        match client
            .search_queue(&regex(Tag::Title, "(", false), 0..1)
            .await
        {
            Err(Error::Ack(ack)) => assert_eq!((ack.index, &*ack.command), (1, "playlistsearch")),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            client.search_queue(&Filter::And(vec![]), 0..1).await,
            Err(Error::InvalidArgument)
        ));
        client.next().await.unwrap();
        drop(client);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn replace_queue_rejects_newline_unsent() {
        let (addr, server) = fake(&[("next\n", "OK\n")]).await;
        let mut client = Client::connect(addr).await.unwrap();
        assert!(matches!(
            client.replace_queue("a\nclear").await,
            Err(Error::InvalidArgument)
        ));
        client.next().await.unwrap();
        drop(client);
        server.await.unwrap();
    }

    #[test]
    fn rejects_newline_argument() {
        let mut out = String::new();
        assert!(matches!(
            encode(&mut out, "password", &["a\nstatus"]),
            Err(Error::InvalidArgument)
        ));
    }
}
