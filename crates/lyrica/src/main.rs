//! lyrica server: serves the embedded PWA and bridges it to MPD.
//!
//! All configuration comes from command-line arguments; the server keeps no
//! on-disk state.

mod assets;

use std::{error::Error, net::SocketAddr};

use axum::{Router, routing::get};
use clap::Parser;
use tracing_subscriber::EnvFilter;

/// Mobile-first PWA client for MPD.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// Address to serve plain HTTP on (terminate TLS in a reverse proxy).
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: SocketAddr,

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

    let app = Router::new().fallback_service(get(assets::serve));

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
