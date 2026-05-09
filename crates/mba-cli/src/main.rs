use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use mba_protocol::StatusResponse;
use reqwest::{Client, Method, Url};

#[derive(Debug, Parser)]
#[command(author, version, about = "Matchbox Audio command-line client")]
struct Cli {
    /// Matchbox Audio daemon base URL.
    #[arg(long, global = true, default_value = "http://127.0.0.1:8090")]
    server: Url,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show daemon status.
    Status,
    /// Resume the current MPD song or start at the head of the queue.
    Play,
    /// Pause MPD playback.
    Pause,
    /// Toggle play and pause.
    Toggle,
    /// Stop MPD playback.
    Stop,
    /// Skip to the next track.
    Next,
    /// Skip to the previous track.
    #[command(alias = "previous")]
    Prev,
    /// Seek to an absolute position in the current track.
    Seek {
        /// Position in seconds (non-negative).
        seconds: f64,
    },
    /// Set MPD volume (0..=100).
    Volume {
        /// Volume level between 0 and 100.
        level: i32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = Client::new();

    match cli.command {
        Command::Status => show_status(&client, cli.server).await,
        Command::Play => post_action(&client, cli.server, "play", None).await,
        Command::Pause => post_action(&client, cli.server, "pause", None).await,
        Command::Toggle => post_action(&client, cli.server, "toggle", None).await,
        Command::Stop => post_action(&client, cli.server, "stop", None).await,
        Command::Next => post_action(&client, cli.server, "next", None).await,
        Command::Prev => post_action(&client, cli.server, "previous", None).await,
        Command::Seek { seconds } => {
            if !seconds.is_finite() || seconds < 0.0 {
                return Err(anyhow!("seconds must be a non-negative finite number"));
            }
            post_action(
                &client,
                cli.server,
                "seek",
                Some(serde_json::json!({ "seconds": seconds })),
            )
            .await
        }
        Command::Volume { level } => {
            if !(0..=100).contains(&level) {
                return Err(anyhow!("level must be between 0 and 100"));
            }
            post_action(
                &client,
                cli.server,
                "volume",
                Some(serde_json::json!({ "level": level })),
            )
            .await
        }
    }
}

async fn show_status(client: &Client, server: Url) -> Result<()> {
    let url = api_url(server, "/api/v1/status");
    let status = client
        .get(url.clone())
        .send()
        .await
        .with_context(|| format!("failed to connect to {url}"))?
        .error_for_status()
        .with_context(|| format!("status request failed for {url}"))?
        .json::<StatusResponse>()
        .await
        .with_context(|| format!("failed to decode status response from {url}"))?;

    println!("service: {}", status.service.name);
    println!("state: {}", status.service.state);
    println!("api: {}", status.service.api_version);
    println!("version: {}", status.build.version);
    if let Some(git_sha) = status.build.git_sha {
        println!("git_sha: {git_sha}");
    }
    if let Some(network) = status.network {
        println!("network_mode: {}", network.mode);
        println!("network_active_connection: {}", network.active_connection);
        println!("network_ssid: {}", network.ssid);
        println!("network_ip4: {}", network.ip4);
        println!("hotspot_ssid: {}", network.hotspot_ssid);
    }
    if let Some(playback) = status.playback {
        println!("playback_state: {}", playback.state);
        println!("playback_volume: {}", playback.volume);
        println!("playback_queue_length: {}", playback.queue_length);
        if let Some(track) = playback.track {
            println!("playback_track_uri: {}", track.uri);
            if let Some(title) = track.title {
                println!("playback_track_title: {title}");
            }
            if let Some(artist) = track.artist {
                println!("playback_track_artist: {artist}");
            }
            if let Some(album) = track.album {
                println!("playback_track_album: {album}");
            }
            if let Some(duration_s) = track.duration_s {
                println!("playback_track_duration_s: {duration_s}");
            }
            if let Some(elapsed_s) = track.elapsed_s {
                println!("playback_track_elapsed_s: {elapsed_s}");
            }
        }
    }

    Ok(())
}

async fn post_action(
    client: &Client,
    server: Url,
    action: &str,
    body: Option<serde_json::Value>,
) -> Result<()> {
    let url = api_url(server, &format!("/api/v1/playback/{action}"));
    let mut request = client.request(Method::POST, url.clone());
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request
        .send()
        .await
        .with_context(|| format!("failed to connect to {url}"))?;

    let status = response.status();
    if status.is_success() {
        return Ok(());
    }

    let body_text = response.text().await.unwrap_or_default();
    Err(anyhow!(
        "playback {action} failed ({status}): {body}",
        body = body_text.trim()
    ))
}

fn api_url(mut server: Url, path: &str) -> Url {
    server.set_path(path);
    server.set_query(None);
    server
}
