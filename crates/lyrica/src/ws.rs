//! Frontend WebSocket endpoint: pushes the player state, accepts commands.
//! Protocol: `docs/architecture.md` → "MPD bridge".

use std::time::Instant;

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use tokio::sync::{mpsc, watch};

use crate::bridge::{Command, Snapshot};

#[derive(Clone)]
pub struct Bridge {
    pub snapshots: watch::Receiver<Snapshot>,
    pub commands: mpsc::Sender<Command>,
}

pub async fn upgrade(ws: WebSocketUpgrade, State(bridge): State<Bridge>) -> Response {
    ws.on_upgrade(|socket| handle(socket, bridge))
}

async fn handle(mut socket: WebSocket, mut bridge: Bridge) {
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
