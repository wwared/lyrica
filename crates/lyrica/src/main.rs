//! lyrica server: serves the embedded PWA and bridges it to MPD.
//!
//! All configuration comes from command-line arguments; the server keeps no
//! on-disk state.

mod assets;
mod bridge;
mod ws;

use std::{error::Error, net::SocketAddr, sync::Arc};

use axum::{Router, routing::get};
use clap::Parser;
use tokio::sync::{mpsc, watch};
use tracing_subscriber::EnvFilter;

use crate::bridge::{Mpd, Snapshot};

/// Mobile-first PWA client for MPD.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// Address to serve plain HTTP on (terminate TLS in a reverse proxy).
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: SocketAddr,

    /// MPD address (TCP only).
    #[arg(long, default_value = "127.0.0.1:6600")]
    mpd: String,

    /// MPD password, sent after every connect.
    #[arg(long)]
    mpd_password: Option<String>,

    /// Base URL of an HTTP mirror of MPD's music directory; enables album
    /// art (`cover.*` in the song's folder), loaded by the browser directly.
    #[arg(long)]
    music_url: Option<String>,

    /// Log filter, in tracing-subscriber `EnvFilter` syntax.
    #[arg(long, default_value = "info")]
    log: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_new(&args.log)?)
        .init();

    let mpd = Arc::new(Mpd {
        addr: args.mpd,
        password: args.mpd_password,
    });
    let (snapshot_tx, snapshots) = watch::channel(Snapshot::offline());
    let (commands, command_rx) = mpsc::channel(16);
    let music_url = args.music_url.map(|mut url| {
        if !url.ends_with('/') {
            url.push('/');
        }
        url
    });
    tokio::spawn(bridge::follow(mpd.clone(), music_url, snapshot_tx));
    tokio::spawn(bridge::execute_commands(mpd, command_rx, snapshots.clone()));

    let app = Router::new()
        .route("/ws", get(ws::upgrade))
        .fallback_service(get(assets::serve))
        .with_state(ws::Bridge {
            snapshots,
            commands,
        });

    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    tracing::info!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}
