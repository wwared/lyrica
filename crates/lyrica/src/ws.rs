//! Frontend WebSocket endpoint: pushes the player state, accepts commands,
//! queue window requests, searches and stored playlist listings and loads.
//! Protocol: `docs/architecture.md` → "MPD bridge" and "Search" → "Server".

use std::{sync::Arc, time::Instant};

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{
    mpsc::{self, error::TrySendError},
    watch,
};

use crate::{
    bridge::{self, Command, Job, Read, ReadRequest, Snapshot, Window},
    query, search,
};

#[derive(Clone)]
pub struct Bridge {
    pub snapshots: watch::Receiver<Snapshot>,
    pub jobs: mpsc::Sender<Job>,
    pub reads: mpsc::Sender<ReadRequest>,
    /// `config` message, sent first on every connection.
    pub config: Arc<str>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ClientMessage {
    Queue(Window),
    Playlists,
    Load {
        name: String,
    },
    Search {
        id: u32,
        query: String,
        from: Option<u32>,
    },
    Matches {
        id: u32,
        query: String,
        start: u32,
    },
    SearchEnd,
    #[serde(untagged)]
    Command(Command),
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ServerMessage<'a> {
    Config { stream: Option<&'a str> },
}

pub fn config_message(stream: Option<&str>) -> String {
    serde_json::to_string(&ServerMessage::Config { stream }).expect("config message serializes")
}

pub async fn upgrade(ws: WebSocketUpgrade, State(bridge): State<Bridge>) -> Response {
    ws.on_upgrade(|socket| handle(socket, bridge))
}

async fn handle(mut socket: WebSocket, mut bridge: Bridge) {
    if socket.send(Message::text(&*bridge.config)).await.is_err() {
        return;
    }
    bridge.snapshots.mark_changed();
    // Unbounded so the MPD tasks never wait on this socket; the request
    // channels' capacities bound it.
    let (reply_tx, mut replies) = mpsc::unbounded_channel();
    loop {
        tokio::select! {
            biased;
            changed = bridge.snapshots.changed() => {
                if changed.is_err() {
                    return;
                }
                let text = bridge.snapshots.borrow_and_update().message(Instant::now());
                if socket.send(Message::text(text)).await.is_err() {
                    return;
                }
            }
            Some(reply) = replies.recv() => {
                if socket.send(Message::text(reply)).await.is_err() {
                    return;
                }
            }
            message = socket.recv() => match message {
                Some(Ok(Message::Text(text))) => match serde_json::from_str(&text) {
                    Ok(ClientMessage::Command(command)) => {
                        if bridge.jobs.send(Job::Command(command)).await.is_err() {
                            return;
                        }
                    }
                    Ok(ClientMessage::Load { name }) => match bridge.jobs.try_reserve() {
                        Ok(permit) => permit.send(Job::Load { name, reply: reply_tx.clone() }),
                        Err(TrySendError::Full(())) => {
                            let _ = reply_tx.send(bridge::loaded(&name, Some("Busy")));
                        }
                        Err(TrySendError::Closed(())) => return,
                    },
                    Ok(ClientMessage::Queue(window)) => {
                        if !read(&bridge, &reply_tx, Read::Window(window)) {
                            return;
                        }
                    }
                    Ok(ClientMessage::Playlists) => {
                        if !read(&bridge, &reply_tx, Read::Playlists) {
                            return;
                        }
                    }
                    Ok(ClientMessage::Search { id, query, from }) => {
                        let request = query::compile(&query).map(|filter| Read::Search { id, filter, from });
                        if !search_read(&bridge, &reply_tx, id, request) {
                            return;
                        }
                    }
                    Ok(ClientMessage::Matches { id, query, start }) => {
                        let request = query::compile(&query).map(|filter| Read::Matches { id, filter, start });
                        if !search_read(&bridge, &reply_tx, id, request) {
                            return;
                        }
                    }
                    Ok(ClientMessage::SearchEnd) => {
                        if !read(&bridge, &reply_tx, Read::SearchEnd) {
                            return;
                        }
                    }
                    Err(error) => tracing::warn!("ignoring message {:?}: {error}", text.as_str()),
                },
                // axum answers pings; a close frame ends the stream.
                Some(Ok(_)) => {}
                Some(Err(_)) | None => return,
            },
        }
    }
}

/// Queues `read` without waiting, replying "unserved" at once when the queue
/// task is busy; false once the task is gone.
fn read(bridge: &Bridge, reply: &mpsc::UnboundedSender<String>, read: Read) -> bool {
    let request = ReadRequest {
        read,
        reply: reply.clone(),
    };
    match bridge.reads.try_send(request) {
        Ok(()) => true,
        Err(TrySendError::Full(request)) => {
            if let Some(text) = request.read.unserved() {
                let _ = reply.send(text);
            }
            true
        }
        Err(TrySendError::Closed(_)) => false,
    }
}

/// `read` for a compiled query; a query error is answered here.
fn search_read(
    bridge: &Bridge,
    reply: &mpsc::UnboundedSender<String>,
    id: u32,
    request: Result<Read, query::QueryError>,
) -> bool {
    match request {
        Ok(request) => read(bridge, reply, request),
        Err(error) => {
            let _ = reply.send(search::failed(id, &error.to_string()));
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::{Router, routing::get};
    use futures_util::{SinkExt, StreamExt};
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::{TcpListener, TcpStream},
    };
    use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite};

    use super::*;
    use crate::bridge::{self, Mpd, PlayState};

    type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

    async fn recv(ws: &mut Client) -> serde_json::Value {
        let message = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("message in time")
            .unwrap()
            .unwrap();
        serde_json::from_str(message.to_text().unwrap()).unwrap()
    }

    async fn send(ws: &mut Client, message: serde_json::Value) {
        let text = tungstenite::Message::text(message.to_string());
        ws.send(text).await.unwrap();
    }

    #[test]
    fn client_messages_route_by_type() {
        let parse = |json| serde_json::from_str::<ClientMessage>(json).map(|m| format!("{m:?}"));
        assert_eq!(
            parse(r#"{"type":"queue","version":3,"start":10,"end":20}"#).unwrap(),
            "Queue(Window { version: 3, start: 10, end: 20 })"
        );
        assert_eq!(
            parse(r#"{"type":"queue","version":3,"start":10,"end":5000}"#).unwrap(),
            "Queue(Window { version: 3, start: 10, end: 210 })"
        );
        assert!(parse(r#"{"type":"queue","version":3,"start":10,"end":9}"#).is_err());
        assert_eq!(parse(r#"{"type":"next"}"#).unwrap(), "Command(Next)");
        assert_eq!(
            parse(r#"{"type":"albumstart"}"#).unwrap(),
            "Command(AlbumStart)"
        );
        assert_eq!(parse(r#"{"type":"playlists"}"#).unwrap(), "Playlists");
        assert_eq!(
            parse(r#"{"type":"load","name":"a \"b\""}"#).unwrap(),
            r#"Load { name: "a \"b\"" }"#
        );
        assert!(parse(r#"{"type":"load"}"#).is_err());
        assert!(parse(r#"{"type":"seek","position":-1}"#).is_err());
        assert!(parse(r#"{"type":"nope"}"#).is_err());
    }

    /// Serves `bridge` and connects past the `config` and first `state`.
    async fn open(bridge: Bridge) -> Client {
        let app = Router::new().route("/ws", get(upgrade)).with_state(bridge);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .unwrap();
        assert_eq!(recv(&mut ws).await["type"], "config");
        assert_eq!(recv(&mut ws).await["type"], "state");
        ws
    }

    #[tokio::test]
    async fn search_replies_over_the_socket() {
        let mpd_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mpd = Arc::new(Mpd {
            addr: mpd_listener.local_addr().unwrap().to_string(),
            password: None,
        });
        tokio::spawn(async move {
            let (stream, _) = mpd_listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            stream.write_all(b"OK MPD 0.24.0\n").await.unwrap();
            let mut list = String::new();
            for _ in 0..4 {
                stream.read_line(&mut list).await.unwrap();
            }
            assert!(
                list.ends_with("\"window\" \"0:500\"\ncommand_list_end\n"),
                "{list}"
            );
            let found = "playlist: 9\nplaylistlength: 50\nfile: a\nPos: 4\nfile: b\nPos: 40\nOK\n";
            stream.write_all(found.as_bytes()).await.unwrap();
            stream
        });
        let playing = Snapshot {
            state: PlayState::Play,
            ..Snapshot::offline()
        };
        let (_snapshot_tx, snapshots) = watch::channel(playing);
        let (jobs, _job_rx) = mpsc::channel(1);
        let (reads, read_rx) = mpsc::channel(2);
        tokio::spawn(bridge::serve_reads(mpd, None, read_rx, snapshots.clone()));
        let mut ws = open(Bridge {
            snapshots,
            jobs,
            reads,
            config: config_message(None).into(),
        })
        .await;
        send(
            &mut ws,
            serde_json::json!({"type": "search", "id": 3, "query": "x", "from": 10}),
        )
        .await;
        assert_eq!(
            recv(&mut ws).await,
            serde_json::json!({"type": "matches", "id": 3, "version": 9, "start": 0,
                               "positions": [4, 40], "first": 1})
        );
    }

    #[tokio::test]
    async fn stalled_window_never_delays_state_or_commands() {
        let mpd_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mpd = Arc::new(Mpd {
            addr: mpd_listener.local_addr().unwrap().to_string(),
            password: None,
        });
        // Reads the first window request, then never answers.
        let stall = tokio::spawn(async move {
            let (stream, _) = mpd_listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            stream.write_all(b"OK MPD 0.24.0\n").await.unwrap();
            let mut line = String::new();
            stream.read_line(&mut line).await.unwrap();
            assert_eq!(line, "playlistinfo \"0:100\"\n");
            stream
        });
        let playing = Snapshot {
            state: PlayState::Play,
            ..Snapshot::offline()
        };
        let (snapshot_tx, snapshots) = watch::channel(playing.clone());
        // One slot, so a queued command leaves no room for a load.
        let (jobs, mut job_rx) = mpsc::channel(1);
        let (reads, read_rx) = mpsc::channel(2);
        tokio::spawn(bridge::serve_reads(mpd, None, read_rx, snapshots.clone()));
        let mut ws = open(Bridge {
            snapshots,
            jobs,
            reads,
            config: config_message(None).into(),
        })
        .await;

        let window = |start: u32| serde_json::json!({"type": "queue", "version": 1, "start": start, "end": start + 100});
        send(&mut ws, window(0)).await;
        let _stalled = stall.await.unwrap();
        // Fills the request channel; the next one is refused at once.
        send(&mut ws, window(100)).await;
        send(&mut ws, window(200)).await;
        send(&mut ws, window(300)).await;
        assert_eq!(
            recv(&mut ws).await,
            serde_json::json!({"type": "queue", "version": 1, "start": 300, "songs": null})
        );
        send(&mut ws, serde_json::json!({"type": "playlists"})).await;
        assert_eq!(
            recv(&mut ws).await,
            serde_json::json!({"type": "playlists", "names": null})
        );
        send(
            &mut ws,
            serde_json::json!({"type": "search", "id": 1, "query": "x", "from": 5}),
        )
        .await;
        assert_eq!(
            recv(&mut ws).await,
            serde_json::json!({"type": "searchfailed", "id": 1, "error": "Busy"})
        );
        send(
            &mut ws,
            serde_json::json!({"type": "matches", "id": 1, "query": "x", "start": 500}),
        )
        .await;
        assert_eq!(
            recv(&mut ws).await,
            serde_json::json!({"type": "matches", "id": 1, "version": null, "start": 500,
                               "positions": null, "first": null})
        );
        // Never reaches the queue task.
        send(
            &mut ws,
            serde_json::json!({"type": "search", "id": 2, "query": " \t", "from": null}),
        )
        .await;
        assert_eq!(
            recv(&mut ws).await,
            serde_json::json!({"type": "searchfailed", "id": 2, "error": "Empty search"})
        );

        snapshot_tx.send_replace(Snapshot {
            title: Some("x".into()),
            ..playing
        });
        assert_eq!(recv(&mut ws).await["title"], "x");
        send(&mut ws, serde_json::json!({"type": "next"})).await;
        send(&mut ws, serde_json::json!({"type": "load", "name": "p"})).await;
        assert_eq!(
            recv(&mut ws).await,
            serde_json::json!({"type": "loaded", "name": "p", "error": "Busy"})
        );
        let job = tokio::time::timeout(Duration::from_secs(5), job_rx.recv())
            .await
            .expect("command in time");
        assert_eq!(format!("{job:?}"), "Some(Command(Next))");
    }

    #[test]
    fn config_message_carries_the_stream_url() {
        let json = |s| serde_json::from_str::<serde_json::Value>(&config_message(s)).unwrap();
        assert_eq!(
            json(Some("https://s/")),
            serde_json::json!({"type": "config", "stream": "https://s/"})
        );
        assert_eq!(
            json(None),
            serde_json::json!({"type": "config", "stream": null})
        );
    }
}
