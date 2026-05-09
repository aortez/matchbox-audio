use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use mba_protocol::{LibraryListing, NetworkInfo, NetworkMode, RescanResponse, StatusResponse};
use serde::Deserialize;
use tokio::{process::Command, time::timeout};
use tracing::warn;

use crate::mpd::{MpdError, MpdHandle};

#[derive(Clone)]
pub struct AppState {
    pub status: StatusResponse,
    pub network_script: PathBuf,
    pub mpd: MpdHandle,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/v1/status", get(status))
        .route("/api/v1/playback/play", post(play))
        .route("/api/v1/playback/pause", post(pause))
        .route("/api/v1/playback/toggle", post(toggle))
        .route("/api/v1/playback/stop", post(stop))
        .route("/api/v1/playback/next", post(next))
        .route("/api/v1/playback/previous", post(previous))
        .route("/api/v1/playback/seek", post(seek))
        .route("/api/v1/playback/volume", post(volume))
        .route("/api/v1/library", get(library))
        .route("/api/v1/library/rescan", post(rescan))
        .with_state(state)
}

async fn index(State(state): State<AppState>) -> Html<String> {
    let network = read_network_status(&state.network_script).await;
    let playback = state.mpd.snapshot();

    let network_mode = network
        .as_ref()
        .map(|network| network.mode.as_str())
        .unwrap_or("unknown");
    let network_address = network
        .as_ref()
        .map(|network| network.ip4.as_str())
        .unwrap_or("-");
    let connection_name = network
        .as_ref()
        .map(|network| network.active_connection.as_str())
        .unwrap_or("-");

    let (playback_state, volume_text, queue_text, track_block) = match playback {
        Some(info) => {
            let track_block = info
                .track
                .as_ref()
                .map(|track| {
                    let title = track.title.as_deref().unwrap_or("(untitled)");
                    let artist = track.artist.as_deref().unwrap_or("");
                    let album = track.album.as_deref().unwrap_or("");
                    let elapsed = track.elapsed_s.unwrap_or(0);
                    let duration = track.duration_s.unwrap_or(0);
                    format!(
                        r#"<p class="track">{title}</p>
    <p class="meta">{artist}{separator}{album}</p>
    <p class="time">{elapsed} / {duration}</p>"#,
                        title = html_escape(title),
                        artist = html_escape(artist),
                        separator = if album.is_empty() || artist.is_empty() {
                            ""
                        } else {
                            " — "
                        },
                        album = html_escape(album),
                        elapsed = format_seconds(elapsed),
                        duration = format_seconds(duration),
                    )
                })
                .unwrap_or_else(|| String::from(r#"<p class="track muted">idle</p>"#));
            (
                info.state.to_string(),
                format!("{}", info.volume),
                format!("{}", info.queue_length),
                track_block,
            )
        }
        None => (
            String::from("unavailable"),
            String::from("-"),
            String::from("-"),
            String::from(r#"<p class="track muted">MPD unavailable</p>"#),
        ),
    };

    Html(format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Matchbox Audio</title>
  <style>
    body {{ font-family: system-ui, sans-serif; margin: 1.25rem; max-width: 32rem; }}
    h1 {{ margin: 0 0 0.5rem; }}
    section {{ border: 1px solid #ddd; border-radius: 0.5rem; padding: 0.75rem 1rem; margin-bottom: 0.75rem; }}
    .row {{ display: flex; gap: 1.5rem; flex-wrap: wrap; }}
    .row > div {{ min-width: 8rem; }}
    .label {{ font-size: 0.75rem; text-transform: uppercase; color: #666; }}
    .value {{ font-size: 1.05rem; }}
    .track {{ font-size: 1.1rem; margin: 0.1rem 0; }}
    .meta {{ color: #444; margin: 0.1rem 0; }}
    .time {{ color: #444; margin: 0.1rem 0; }}
    .muted {{ color: #888; font-style: italic; }}
  </style>
</head>
<body>
  <h1>Matchbox Audio</h1>

  <section>
    <div class="row">
      <div><div class="label">service</div><div class="value">{service_state}</div></div>
      <div><div class="label">version</div><div class="value">{version}</div></div>
    </div>
  </section>

  <section>
    <div class="row">
      <div><div class="label">network</div><div class="value">{network_mode}</div></div>
      <div><div class="label">connection</div><div class="value">{connection}</div></div>
      <div><div class="label">address</div><div class="value">{address}</div></div>
    </div>
  </section>

  <section>
    <div class="row">
      <div><div class="label">playback</div><div class="value">{playback_state}</div></div>
      <div><div class="label">volume</div><div class="value">{volume}</div></div>
      <div><div class="label">queue</div><div class="value">{queue}</div></div>
    </div>
    {track_block}
  </section>

  <p><small>API: <code>/api/v1/status</code></small></p>
</body>
</html>
"#,
        service_state = html_escape(&state.status.service.state.to_string()),
        version = html_escape(&state.status.build.version),
        network_mode = html_escape(network_mode),
        connection = html_escape(connection_name),
        address = html_escape(network_address),
        playback_state = html_escape(&playback_state),
        volume = html_escape(&volume_text),
        queue = html_escape(&queue_text),
        track_block = track_block,
    ))
}

fn format_seconds(seconds: u32) -> String {
    let minutes = seconds / 60;
    let remainder = seconds % 60;
    format!("{minutes}:{remainder:02}")
}

fn html_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            other => out.push(other),
        }
    }
    out
}

async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    let mut status = state.status.clone();
    status.network = read_network_status(&state.network_script).await;
    status.playback = state.mpd.snapshot();
    Json(status)
}

async fn play(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state.mpd.play().await?;
    Ok(StatusCode::ACCEPTED)
}

async fn pause(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state.mpd.pause().await?;
    Ok(StatusCode::ACCEPTED)
}

async fn toggle(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state.mpd.toggle().await?;
    Ok(StatusCode::ACCEPTED)
}

async fn stop(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state.mpd.stop().await?;
    Ok(StatusCode::ACCEPTED)
}

async fn next(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state.mpd.next().await?;
    Ok(StatusCode::ACCEPTED)
}

async fn previous(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state.mpd.previous().await?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Debug, Deserialize)]
struct SeekRequest {
    seconds: f64,
}

async fn seek(
    State(state): State<AppState>,
    Json(req): Json<SeekRequest>,
) -> Result<StatusCode, ApiError> {
    if !req.seconds.is_finite() || req.seconds < 0.0 {
        return Err(ApiError::bad_request(
            "seconds must be a non-negative finite number",
        ));
    }
    state.mpd.seek(req.seconds).await?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Debug, Deserialize)]
struct VolumeRequest {
    level: i32,
}

async fn volume(
    State(state): State<AppState>,
    Json(req): Json<VolumeRequest>,
) -> Result<StatusCode, ApiError> {
    if !(0..=100).contains(&req.level) {
        return Err(ApiError::bad_request("level must be between 0 and 100"));
    }
    state.mpd.set_volume(req.level as u8).await?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Debug, Deserialize)]
struct LibraryQuery {
    #[serde(default)]
    path: String,
}

async fn library(
    State(state): State<AppState>,
    Query(query): Query<LibraryQuery>,
) -> Result<Json<LibraryListing>, ApiError> {
    let listing = state.mpd.list_library(query.path).await?;
    Ok(Json(listing))
}

async fn rescan(State(state): State<AppState>) -> Result<Json<RescanResponse>, ApiError> {
    let job_id = state.mpd.rescan().await?;
    Ok(Json(RescanResponse { job_id }))
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
}

impl From<MpdError> for ApiError {
    fn from(error: MpdError) -> Self {
        let status = match &error {
            MpdError::Unavailable | MpdError::ChannelClosed => StatusCode::SERVICE_UNAVAILABLE,
            MpdError::Command(_) => StatusCode::BAD_GATEWAY,
        };
        Self {
            status,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

async fn read_network_status(network_script: &Path) -> Option<NetworkInfo> {
    let stdout = match run_network_status_command(network_script).await {
        Ok(stdout) => stdout,
        Err(error) => {
            warn!(
                %error,
                path = %network_script.display(),
                "network status helper failed"
            );
            return None;
        }
    };

    Some(NetworkInfo {
        mode: NetworkMode::parse(parse_field(&stdout, "mode").unwrap_or("unknown")),
        active_connection: parse_field(&stdout, "active_connection")
            .unwrap_or("none")
            .to_string(),
        ssid: parse_field(&stdout, "ssid").unwrap_or("none").to_string(),
        ip4: parse_field(&stdout, "ip4").unwrap_or("none").to_string(),
        hotspot_ssid: parse_field(&stdout, "hotspot_ssid")
            .unwrap_or("matchbox-audio")
            .to_string(),
    })
}

async fn run_network_status_command(network_script: &Path) -> Result<String, String> {
    let mut command = Command::new(network_script);
    command.arg("status");

    let output = match timeout(Duration::from_secs(5), command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return Err(format!("failed to run helper: {error}")),
        Err(_) => return Err("helper timed out".to_string()),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "helper exited with {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_field<'a>(output: &'a str, field: &str) -> Option<&'a str> {
    let prefix = format!("{field}=");
    output
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
