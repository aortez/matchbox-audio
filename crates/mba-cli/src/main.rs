use std::{io::Write as _, path::PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use mba_protocol::{
    BtClientRecord, BtControlRequest, BtControlResponse, BtStatus, LibraryListing, QueueListing,
    RescanResponse, StatusResponse, DEFAULT_BT_CONTROL_SOCKET, DEFAULT_BT_PAIRING_TIMEOUT_SECONDS,
    MAX_BT_PAIRING_TIMEOUT_SECONDS,
};
use reqwest::{Client, Method, Url};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

const DEFAULT_SNAPSHOT_SOCKET: &str = "/run/mba-device/snapshot.sock";

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
    /// List a folder in the music library.
    #[command(alias = "library")]
    List {
        /// Library path relative to the music root. Empty for the root.
        #[arg(default_value = "")]
        path: String,
    },
    /// Trigger an MPD library rescan.
    Rescan,
    /// Enqueue a track by library path.
    #[command(alias = "enqueue-file")]
    Enqueue {
        /// Track path relative to the music root.
        path: String,
    },
    /// Enqueue a directory by library path.
    EnqueueDir {
        /// Directory path relative to the music root.
        path: String,
    },
    /// Show the current playback queue.
    Queue,
    /// Remove an item from the current playback queue.
    QueueRemove {
        /// Stable MPD queue id. Preferred when available from `mba-cli queue`.
        #[arg(long)]
        id: Option<u64>,
        /// Zero-based queue position.
        position: Option<u32>,
    },
    /// Move an item to a zero-based queue position.
    QueueMove {
        /// Stable MPD queue id. Preferred when available from `mba-cli queue`.
        #[arg(long)]
        id: Option<u64>,
        /// Zero-based destination queue position for `--id`.
        #[arg(long, value_name = "POSITION")]
        to: Option<u32>,
        /// Position arguments. Use `<from> <to>`, or `<to>` with `--id`.
        #[arg(value_name = "POSITION")]
        positions: Vec<u32>,
    },
    /// Move an item immediately after the current track.
    QueueNext {
        /// Stable MPD queue id. Preferred when available from `mba-cli queue`.
        #[arg(long)]
        id: Option<u64>,
        /// Zero-based queue position.
        position: Option<u32>,
    },
    /// Clear the current playback queue.
    Clear,
    /// Capture the current LCD framebuffer as a PNG.
    Screenshot {
        /// Output file path. Use `-` for stdout.
        #[arg(short, long, default_value = "-")]
        output: String,
        /// Path to the mba-device snapshot socket.
        #[arg(long, default_value = DEFAULT_SNAPSHOT_SOCKET)]
        socket: PathBuf,
    },
    /// Inspect or control the local Bluetooth daemon.
    Bt {
        /// Path to the mba-bt local control socket.
        #[arg(long, default_value = DEFAULT_BT_CONTROL_SOCKET)]
        socket: PathBuf,
        #[command(subcommand)]
        command: BtCommand,
    },
}

#[derive(Debug, Subcommand)]
enum BtCommand {
    /// Show local Bluetooth daemon status.
    Status,
    /// List trusted Bluetooth app clients.
    Clients,
    /// Forget one trusted Bluetooth app client.
    Forget {
        /// Client id from `mba-cli bt clients`.
        client_id: String,
    },
    /// Open or close the local Bluetooth pairing window.
    Pairing {
        #[command(subcommand)]
        command: BtPairingCommand,
    },
}

#[derive(Debug, Subcommand)]
enum BtPairingCommand {
    /// Temporarily allow a new app client to pair.
    Start {
        /// Pairing window length in seconds.
        #[arg(long, default_value_t = DEFAULT_BT_PAIRING_TIMEOUT_SECONDS)]
        timeout: u64,
    },
    /// Close the current pairing window.
    Stop,
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
        Command::List { path } => list_library(&client, cli.server, path).await,
        Command::Rescan => trigger_rescan(&client, cli.server).await,
        Command::Enqueue { path } => enqueue_path(&client, cli.server, "files", path).await,
        Command::EnqueueDir { path } => {
            enqueue_path(&client, cli.server, "directories", path).await
        }
        Command::Queue => show_queue(&client, cli.server).await,
        Command::QueueRemove { id, position } => {
            let body = queue_target_body(id, position)?;
            post_queue_edit(&client, cli.server, "remove", body).await
        }
        Command::QueueMove { id, to, positions } => {
            let body = queue_move_body(id, to, positions)?;
            post_queue_edit(&client, cli.server, "move", body).await
        }
        Command::QueueNext { id, position } => {
            let body = queue_target_body(id, position)?;
            post_queue_edit(&client, cli.server, "play-next", body).await
        }
        Command::Clear => clear_queue(&client, cli.server).await,
        Command::Screenshot { output, socket } => capture_screenshot(socket, output).await,
        Command::Bt { socket, command } => match command {
            BtCommand::Status => show_bt_status(socket).await,
            BtCommand::Clients => show_bt_clients(socket).await,
            BtCommand::Forget { client_id } => forget_bt_client(socket, client_id).await,
            BtCommand::Pairing { command } => match command {
                BtPairingCommand::Start { timeout } => start_bt_pairing(socket, timeout).await,
                BtPairingCommand::Stop => stop_bt_pairing(socket).await,
            },
        },
    }
}

async fn show_bt_status(socket: PathBuf) -> Result<()> {
    let response = send_bt_control_request(&socket, &BtControlRequest::status()).await?;
    let status = bt_response_status("bt status", response)?;

    print!("{}", format_bt_status(&status));
    Ok(())
}

async fn start_bt_pairing(socket: PathBuf, timeout: u64) -> Result<()> {
    validate_bt_pairing_timeout(timeout)?;
    let request = BtControlRequest::pairing_start(timeout);
    let response = send_bt_control_request(&socket, &request).await?;
    let status = bt_response_status("bt pairing start", response)?;

    print!("{}", format_bt_status(&status));
    Ok(())
}

async fn stop_bt_pairing(socket: PathBuf) -> Result<()> {
    let response = send_bt_control_request(&socket, &BtControlRequest::pairing_stop()).await?;
    let status = bt_response_status("bt pairing stop", response)?;

    print!("{}", format_bt_status(&status));
    Ok(())
}

async fn show_bt_clients(socket: PathBuf) -> Result<()> {
    let response = send_bt_control_request(&socket, &BtControlRequest::clients()).await?;
    let clients = bt_response_clients("bt clients", response)?;

    print!("{}", format_bt_clients(&clients));
    Ok(())
}

async fn forget_bt_client(socket: PathBuf, client_id: String) -> Result<()> {
    if client_id.trim().is_empty() {
        return Err(anyhow!("client_id must not be empty"));
    }
    let request = BtControlRequest::forget_client(&client_id);
    let response = send_bt_control_request(&socket, &request).await?;
    let status = bt_response_status("bt forget", response)?;

    println!("forgot_client: {client_id}");
    print!("{}", format_bt_status(&status));
    Ok(())
}

fn validate_bt_pairing_timeout(timeout: u64) -> Result<()> {
    if !(1..=MAX_BT_PAIRING_TIMEOUT_SECONDS).contains(&timeout) {
        return Err(anyhow!(
            "timeout must be between 1 and {MAX_BT_PAIRING_TIMEOUT_SECONDS} seconds"
        ));
    }
    Ok(())
}

async fn send_bt_control_request(
    socket: &PathBuf,
    request: &BtControlRequest,
) -> Result<BtControlResponse> {
    let mut stream = UnixStream::connect(socket).await.with_context(|| {
        format!(
            "failed to connect to bt control socket {}",
            socket.display()
        )
    })?;
    let request_bytes = serde_json::to_vec(request).context("failed to encode bt request")?;
    stream
        .write_all(&request_bytes)
        .await
        .with_context(|| format!("failed to write bt request to {}", socket.display()))?;
    stream
        .shutdown()
        .await
        .with_context(|| format!("failed to finish bt request to {}", socket.display()))?;

    let mut response_bytes = Vec::new();
    stream
        .read_to_end(&mut response_bytes)
        .await
        .with_context(|| format!("failed to read bt response from {}", socket.display()))?;
    serde_json::from_slice(&response_bytes)
        .with_context(|| format!("failed to decode bt response from {}", socket.display()))
}

fn bt_response_status(action: &str, response: BtControlResponse) -> Result<BtStatus> {
    if response.ok {
        return response
            .status
            .ok_or_else(|| anyhow!("{action} response was missing status payload"));
    }

    let error = response
        .error
        .ok_or_else(|| anyhow!("{action} failed without an error payload"))?;
    Err(anyhow!(
        "{action} failed: {}: {}",
        error.code,
        error.message
    ))
}

fn bt_response_clients(action: &str, response: BtControlResponse) -> Result<Vec<BtClientRecord>> {
    if response.ok {
        return response
            .clients
            .ok_or_else(|| anyhow!("{action} response was missing clients payload"));
    }

    let error = response
        .error
        .ok_or_else(|| anyhow!("{action} failed without an error payload"))?;
    Err(anyhow!(
        "{action} failed: {}: {}",
        error.code,
        error.message
    ))
}

fn format_bt_status(status: &BtStatus) -> String {
    let mut output = String::new();
    output.push_str(&format!("service: {}\n", status.service));
    output.push_str(&format!("transport: {}\n", status.transport));
    output.push_str(&format!("device_name: {}\n", status.device_name));
    if let Some(state_dir) = &status.state_dir {
        output.push_str(&format!("state_dir: {state_dir}\n"));
    }
    output.push_str(&format!("trusted_clients: {}\n", status.trusted_clients));
    if let Some(adapter) = &status.adapter {
        output.push_str(&format!("adapter: {}\n", adapter.name));
        output.push_str(&format!("adapter_address: {}\n", adapter.address));
    }
    output.push_str(&format!("advertising: {}\n", status.advertising));
    output.push_str(&format!("service_uuid: {}\n", status.service_uuid));
    output.push_str(&format!("pairing_state: {}\n", status.pairing_state));
    if let Some(remaining_seconds) = status.pairing_remaining_seconds {
        output.push_str(&format!("pairing_remaining_seconds: {remaining_seconds}\n"));
    }
    output.push_str(&format!("busy: {}\n", status.busy));
    if let Some(client) = &status.active_client {
        output.push_str(&format!("active_client_address: {}\n", client.address));
        output.push_str(&format!("active_client_adapter: {}\n", client.adapter));
        output.push_str(&format!("active_client_mtu: {}\n", client.mtu));
        output.push_str(&format!(
            "active_client_session_token: {}\n",
            client.session_token
        ));
    }
    output.push_str(&format!("rx_chunk_writes: {}\n", status.rx_chunk_writes));
    output.push_str(&format!("tx_chunks_sent: {}\n", status.tx_chunks_sent));
    output
}

fn format_bt_clients(clients: &[BtClientRecord]) -> String {
    if clients.is_empty() {
        return "(no trusted clients)\n".to_string();
    }

    let mut output = String::new();
    for (index, client) in clients.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        output.push_str(&format!("client_id: {}\n", client.client_id));
        if let Some(display_name) = &client.display_name {
            output.push_str(&format!("display_name: {display_name}\n"));
        }
        output.push_str(&format!("trusted: {}\n", client.trusted));
        output.push_str(&format!(
            "created_unix_seconds: {}\n",
            client.created_unix_seconds
        ));
        if let Some(last_seen_unix_seconds) = client.last_seen_unix_seconds {
            output.push_str(&format!(
                "last_seen_unix_seconds: {last_seen_unix_seconds}\n"
            ));
        }
        if let Some(last_ble_address) = &client.last_ble_address {
            output.push_str(&format!("last_ble_address: {last_ble_address}\n"));
        }
        if let Some(protocol_version) = client.protocol_version {
            output.push_str(&format!("protocol_version: {protocol_version}\n"));
        }
    }
    output
}

async fn capture_screenshot(socket: PathBuf, output: String) -> Result<()> {
    let mut stream = UnixStream::connect(&socket)
        .await
        .with_context(|| format!("failed to connect to snapshot socket {}", socket.display()))?;
    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .await
        .with_context(|| format!("failed to read snapshot from {}", socket.display()))?;
    if bytes.is_empty() {
        return Err(anyhow!("device returned an empty snapshot"));
    }
    if output == "-" {
        let mut stdout = std::io::stdout().lock();
        stdout
            .write_all(&bytes)
            .context("failed to write snapshot to stdout")?;
        stdout.flush().context("failed to flush stdout")?;
    } else {
        std::fs::write(&output, &bytes)
            .with_context(|| format!("failed to write snapshot to {output}"))?;
        eprintln!("wrote {} bytes to {output}", bytes.len());
    }
    Ok(())
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

async fn list_library(client: &Client, server: Url, path: String) -> Result<()> {
    let mut url = api_url(server, "/api/v1/library/list");
    if !path.is_empty() {
        url.query_pairs_mut().append_pair("path", &path);
    }
    let listing = client
        .get(url.clone())
        .send()
        .await
        .with_context(|| format!("failed to connect to {url}"))?
        .error_for_status()
        .with_context(|| format!("library request failed for {url}"))?
        .json::<LibraryListing>()
        .await
        .with_context(|| format!("failed to decode library response from {url}"))?;

    println!("path: {}", listing.path);
    if listing.directories.is_empty() && listing.tracks.is_empty() {
        println!("(empty)");
        return Ok(());
    }
    for dir in &listing.directories {
        println!("dir: {}", dir.name);
    }
    for track in &listing.tracks {
        let title = track.title.as_deref().unwrap_or(&track.name);
        match (track.artist.as_deref(), track.duration_s) {
            (Some(artist), Some(seconds)) => {
                println!("track: {title} — {artist} [{}]", format_seconds(seconds))
            }
            (Some(artist), None) => println!("track: {title} — {artist}"),
            (None, Some(seconds)) => {
                println!("track: {title} [{}]", format_seconds(seconds))
            }
            (None, None) => println!("track: {title}"),
        }
    }
    Ok(())
}

async fn trigger_rescan(client: &Client, server: Url) -> Result<()> {
    let url = api_url(server, "/api/v1/library/rescan");
    let response = client
        .post(url.clone())
        .send()
        .await
        .with_context(|| format!("failed to connect to {url}"))?;

    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "rescan failed ({status}): {body}",
            body = body_text.trim()
        ));
    }
    let payload = response
        .json::<RescanResponse>()
        .await
        .with_context(|| format!("failed to decode rescan response from {url}"))?;
    println!("scan started: job {}", payload.job_id);
    Ok(())
}

async fn enqueue_path(client: &Client, server: Url, kind: &str, path: String) -> Result<()> {
    if path.trim().is_empty() {
        return Err(anyhow!("path must not be empty"));
    }
    let url = api_url(server, &format!("/api/v1/queue/{kind}"));
    let response = client
        .post(url.clone())
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await
        .with_context(|| format!("failed to connect to {url}"))?;

    let status = response.status();
    if status.is_success() {
        return Ok(());
    }

    let body_text = response.text().await.unwrap_or_default();
    Err(anyhow!(
        "enqueue {kind} failed ({status}): {body}",
        body = body_text.trim()
    ))
}

async fn show_queue(client: &Client, server: Url) -> Result<()> {
    let url = api_url(server, "/api/v1/queue");
    let queue = client
        .get(url.clone())
        .send()
        .await
        .with_context(|| format!("failed to connect to {url}"))?
        .error_for_status()
        .with_context(|| format!("queue request failed for {url}"))?
        .json::<QueueListing>()
        .await
        .with_context(|| format!("failed to decode queue response from {url}"))?;

    println!("items: {}", queue.items.len());
    if queue.items.is_empty() {
        println!("(empty)");
        return Ok(());
    }
    for item in &queue.items {
        let title = item.title.as_deref().unwrap_or(&item.name);
        let label = match (item.artist.as_deref(), item.duration_s) {
            (Some(artist), Some(seconds)) => {
                format!("{title} — {artist} [{}]", format_seconds(seconds))
            }
            (Some(artist), None) => format!("{title} — {artist}"),
            (None, Some(seconds)) => format!("{title} [{}]", format_seconds(seconds)),
            (None, None) => title.to_string(),
        };
        match item.id {
            Some(id) => println!("{}: {label} ({}) [id:{id}]", item.position, item.uri),
            None => println!("{}: {label} ({})", item.position, item.uri),
        }
    }
    Ok(())
}

fn queue_target_body(id: Option<u64>, position: Option<u32>) -> Result<serde_json::Value> {
    if id.is_none() && position.is_none() {
        return Err(anyhow!("queue item target requires --id or position"));
    }

    let mut body = serde_json::Map::new();
    if let Some(position) = position {
        body.insert("position".to_string(), serde_json::json!(position));
    }
    if let Some(id) = id {
        body.insert("id".to_string(), serde_json::json!(id));
    }
    Ok(serde_json::Value::Object(body))
}

fn queue_move_body(
    id: Option<u64>,
    to: Option<u32>,
    positions: Vec<u32>,
) -> Result<serde_json::Value> {
    let (position, to_position) = match (id, to, positions.as_slice()) {
        (Some(_), Some(to_position), []) => (None, to_position),
        (Some(_), None, [to_position]) => (None, *to_position),
        (Some(_), Some(_), [_]) => {
            return Err(anyhow!(
                "use either --to or a positional destination, not both"
            ));
        }
        (Some(_), _, _) => {
            return Err(anyhow!(
                "queue-move with --id requires exactly one destination position"
            ));
        }
        (None, None, [position, to_position]) => (Some(*position), *to_position),
        (None, Some(to_position), [position]) => (Some(*position), to_position),
        (None, Some(_), []) => {
            return Err(anyhow!(
                "queue-move without --id requires a source position"
            ));
        }
        (None, _, _) => {
            return Err(anyhow!(
                "queue-move requires <from> <to>, or --id <id> <to>"
            ));
        }
    };

    let mut body = queue_target_body(id, position)?;
    body["to_position"] = serde_json::json!(to_position);
    Ok(body)
}

async fn post_queue_edit(
    client: &Client,
    server: Url,
    action: &str,
    body: serde_json::Value,
) -> Result<()> {
    let url = api_url(server, &format!("/api/v1/queue/{action}"));
    let response = client
        .post(url.clone())
        .json(&body)
        .send()
        .await
        .with_context(|| format!("failed to connect to {url}"))?;

    let status = response.status();
    if status.is_success() {
        return Ok(());
    }

    let body_text = response.text().await.unwrap_or_default();
    Err(anyhow!(
        "queue {action} failed ({status}): {body}",
        body = body_text.trim()
    ))
}

async fn clear_queue(client: &Client, server: Url) -> Result<()> {
    let url = api_url(server, "/api/v1/queue");
    let response = client
        .request(Method::DELETE, url.clone())
        .send()
        .await
        .with_context(|| format!("failed to connect to {url}"))?;

    let status = response.status();
    if status.is_success() {
        return Ok(());
    }

    let body_text = response.text().await.unwrap_or_default();
    Err(anyhow!(
        "clear queue failed ({status}): {body}",
        body = body_text.trim()
    ))
}

fn format_seconds(seconds: u32) -> String {
    let minutes = seconds / 60;
    let remainder = seconds % 60;
    format!("{minutes}:{remainder:02}")
}

fn api_url(mut server: Url, path: &str) -> Url {
    server.set_path(path);
    server.set_query(None);
    server
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_target_body_prefers_id_but_keeps_position_fallback() {
        let body = queue_target_body(Some(42), Some(3)).expect("body");

        assert_eq!(body["id"], 42);
        assert_eq!(body["position"], 3);
    }

    #[test]
    fn queue_target_body_requires_a_target() {
        assert!(queue_target_body(None, None).is_err());
    }

    #[test]
    fn queue_move_body_accepts_position_pair() {
        let body = queue_move_body(None, None, vec![2, 0]).expect("body");

        assert_eq!(body["position"], 2);
        assert_eq!(body["to_position"], 0);
    }

    #[test]
    fn queue_move_body_accepts_id_and_destination() {
        let body = queue_move_body(Some(99), None, vec![1]).expect("body");

        assert_eq!(body["id"], 99);
        assert_eq!(body["to_position"], 1);
        assert!(body.get("position").is_none());
    }

    #[test]
    fn format_bt_status_includes_session_details() {
        let status = BtStatus {
            service: "matchbox-audio".to_string(),
            transport: "mba-bt-ble-local".to_string(),
            device_name: "Matchbox Audio".to_string(),
            state_dir: Some("/data/matchbox-audio/bt".to_string()),
            trusted_clients: 0,
            adapter: Some(mba_protocol::BtAdapterStatus {
                name: "hci0".to_string(),
                address: "88:A2:9E:B1:87:91".to_string(),
            }),
            advertising: true,
            service_uuid: "1cef04f1-966e-43ad-860f-086db4f277d6".to_string(),
            pairing_state: "open".to_string(),
            pairing_remaining_seconds: Some(120),
            busy: true,
            active_client: Some(mba_protocol::BtActiveClientStatus {
                address: "6A:2E:A9:9C:0A:81".to_string(),
                adapter: "hci0".to_string(),
                mtu: 512,
                session_token: 3,
            }),
            rx_chunk_writes: 2,
            tx_chunks_sent: 6,
        };

        let output = format_bt_status(&status);

        assert!(output.contains("service: matchbox-audio\n"));
        assert!(output.contains("state_dir: /data/matchbox-audio/bt\n"));
        assert!(output.contains("trusted_clients: 0\n"));
        assert!(output.contains("adapter_address: 88:A2:9E:B1:87:91\n"));
        assert!(output.contains("pairing_state: open\n"));
        assert!(output.contains("pairing_remaining_seconds: 120\n"));
        assert!(output.contains("busy: true\n"));
        assert!(output.contains("active_client_mtu: 512\n"));
        assert!(output.contains("tx_chunks_sent: 6\n"));
    }

    #[test]
    fn bt_response_status_surfaces_error_payload() {
        let error = bt_response_status(
            "bt pairing start",
            BtControlResponse::error("bad_request", "nope"),
        )
        .expect_err("error response fails");

        assert!(error
            .to_string()
            .contains("bt pairing start failed: bad_request: nope"));
    }

    #[test]
    fn format_bt_clients_handles_empty_and_known_clients() {
        assert_eq!(format_bt_clients(&[]), "(no trusted clients)\n");

        let clients = vec![BtClientRecord {
            schema_version: 1,
            client_id: "phone-1".to_string(),
            display_name: Some("Pixel 7 Pro".to_string()),
            trusted: true,
            created_unix_seconds: 1_765_000_000,
            last_seen_unix_seconds: Some(1_765_000_120),
            last_ble_address: Some("57:29:36:B6:FD:53".to_string()),
            protocol_version: Some(1),
        }];

        let output = format_bt_clients(&clients);

        assert!(output.contains("client_id: phone-1\n"));
        assert!(output.contains("display_name: Pixel 7 Pro\n"));
        assert!(output.contains("trusted: true\n"));
        assert!(output.contains("last_ble_address: 57:29:36:B6:FD:53\n"));
        assert!(output.contains("protocol_version: 1\n"));
    }

    #[test]
    fn bt_response_clients_surfaces_error_payload() {
        let error = bt_response_clients("bt clients", BtControlResponse::error("internal", "nope"))
            .expect_err("error response fails");

        assert!(error
            .to_string()
            .contains("bt clients failed: internal: nope"));
    }

    #[test]
    fn validate_bt_pairing_timeout_bounds() {
        assert!(validate_bt_pairing_timeout(1).is_ok());
        assert!(validate_bt_pairing_timeout(MAX_BT_PAIRING_TIMEOUT_SECONDS).is_ok());
        assert!(validate_bt_pairing_timeout(0).is_err());
        assert!(validate_bt_pairing_timeout(MAX_BT_PAIRING_TIMEOUT_SECONDS + 1).is_err());
    }
}
