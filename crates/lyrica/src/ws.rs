//! Frontend WebSocket endpoint. Placeholder: greets the client, ignores input.

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::Response,
};

pub async fn upgrade(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(handle)
}

async fn handle(mut socket: WebSocket) {
    let hello = concat!("lyrica ", env!("CARGO_PKG_VERSION"));
    if socket.send(Message::text(hello)).await.is_err() {
        return;
    }
    // Keep reading so pings and close frames are handled.
    while let Some(Ok(_)) = socket.recv().await {}
}
