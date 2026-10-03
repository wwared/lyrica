//! Frontend WebSocket endpoint: pushes the player state, accepts commands.
//! Protocol: `docs/architecture.md` → "MPD bridge".

use std::{sync::Arc, time::Instant};

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use serde::Serialize;
use tokio::sync::{mpsc, watch};

use crate::bridge::{Command, Snapshot};

#[derive(Clone)]
pub struct Bridge {
    pub snapshots: watch::Receiver<Snapshot>,
    pub commands: mpsc::Sender<Command>,
    /// `config` message, sent first on every connection.
    pub config: Arc<str>,
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
    loop {
        tokio::select! {
            changed = bridge.snapshots.changed() => {
                if changed.is_err() {
                    return;
                }
                let text = bridge.snapshots.borrow_and_update().message(Instant::now());
                if socket.send(Message::text(text)).await.is_err() {
                    return;
                }
            }
            message = socket.recv() => match message {
                Some(Ok(Message::Text(text))) => match serde_json::from_str(&text) {
                    Ok(command) => {
                        if bridge.commands.send(command).await.is_err() {
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

#[cfg(test)]
mod tests {
    use super::*;

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
