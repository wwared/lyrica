//! Frontend WebSocket endpoint: pushes the player state, accepts commands and
//! queue window requests. Protocol: `docs/architecture.md` → "MPD bridge".

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

use crate::bridge::{Command, Snapshot, Window, WindowRequest};

#[derive(Clone)]
pub struct Bridge {
    pub snapshots: watch::Receiver<Snapshot>,
    pub commands: mpsc::Sender<Command>,
    pub windows: mpsc::Sender<WindowRequest>,
    /// `config` message, sent first on every connection.
    pub config: Arc<str>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ClientMessage {
    Queue(Window),
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
    // Unbounded so the queue task never waits on this socket; the request
    // channel's capacity bounds it.
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
                        if bridge.commands.send(command).await.is_err() {
                            return;
                        }
                    }
                    Ok(ClientMessage::Queue(window)) => {
                        let request = WindowRequest { window, reply: reply_tx.clone() };
                        match bridge.windows.try_send(request) {
                            Ok(()) => {}
                            Err(TrySendError::Full(_)) => {
                                let _ = reply_tx.send(window.unserved());
                            }
                            Err(TrySendError::Closed(_)) => return,
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
        assert!(parse(r#"{"type":"seek","position":-1}"#).is_err());
        assert!(parse(r#"{"type":"nope"}"#).is_err());
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
        let (commands, mut command_rx) = mpsc::channel(16);
        let (windows, window_rx) = mpsc::channel(2);
        tokio::spawn(bridge::serve_windows(
            mpd,
            None,
            window_rx,
            snapshots.clone(),
        ));
        let app = Router::new().route("/ws", get(upgrade)).with_state(Bridge {
            snapshots,
            commands,
            windows,
            config: config_message(None).into(),
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .unwrap();
        assert_eq!(recv(&mut ws).await["type"], "config");
        assert_eq!(recv(&mut ws).await["type"], "state");

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

        snapshot_tx.send_replace(Snapshot {
            title: Some("x".into()),
            ..playing
        });
        assert_eq!(recv(&mut ws).await["title"], "x");
        send(&mut ws, serde_json::json!({"type": "next"})).await;
        let command = tokio::time::timeout(Duration::from_secs(5), command_rx.recv())
            .await
            .expect("command in time");
        assert_eq!(format!("{command:?}"), "Some(Next)");
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
