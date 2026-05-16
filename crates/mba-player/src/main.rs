mod config;
mod http;
mod library;
mod mpd;

use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use config::PlayerConfig;
use http::AppState;
use mba_protocol::StatusResponse;
use tokio::net::TcpListener;
use tracing::info;

#[derive(Debug, Parser)]
#[command(author, version, about = "Matchbox Audio player daemon")]
struct Args {
    /// Path to a JSON player config file.
    #[arg(long)]
    config: Option<PathBuf>,

    /// Address and port for the HTTP API.
    #[arg(long)]
    bind: Option<SocketAddr>,

    /// Address of the MPD server.
    #[arg(long)]
    mpd_addr: Option<SocketAddr>,

    /// Path to the local music library root.
    #[arg(long)]
    music_root: Option<PathBuf>,

    /// Path to the network-mode helper script.
    #[arg(long, default_value = "/usr/bin/mba-network-mode")]
    network_script: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let args = Args::parse();
    let mut config = PlayerConfig::load(args.config.as_deref())?;
    if let Some(bind) = args.bind {
        config.bind = bind;
    }
    if let Some(mpd_addr) = args.mpd_addr {
        config.mpd_addr = mpd_addr;
    }
    if let Some(music_root) = args.music_root {
        config.music_root = music_root;
    }

    let status = StatusResponse::ready(
        env!("CARGO_PKG_VERSION"),
        option_env!("MATCHBOX_AUDIO_GIT_SHA"),
    );
    let (mpd, _mpd_task) = mpd::start(config.mpd_addr);
    info!(addr = %config.mpd_addr, "MPD client task started");
    let app = http::router(AppState {
        status,
        network_script: args.network_script,
        library: library::LibraryBrowser::new(config.music_root),
        mpd,
    });
    let listener = TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("failed to bind HTTP listener on {}", config.bind))?;

    info!(addr = %listener.local_addr()?, "mba-player listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP server failed")?;

    Ok(())
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mba_player=info,tower_http=info".into()),
        )
        .init();
}

async fn shutdown_signal() {
    match tokio::signal::ctrl_c().await {
        Ok(()) => info!("shutdown signal received"),
        Err(error) => tracing::error!(%error, "failed to listen for shutdown signal"),
    }
}
