use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use axum::{
    extract::{Query, State},
    http::{header::CONTENT_TYPE, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use mba_protocol::{
    LibraryListing, NetworkInfo, NetworkMode, QueueListing, RescanResponse, StatusResponse,
};
use serde::Deserialize;
use tokio::{process::Command, time::timeout};
use tracing::warn;

use crate::{
    library::{LibraryBrowser, LibraryError},
    mpd::{MpdError, MpdHandle, QueueItemTarget},
};

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_CSS: &str = include_str!("../web/app.css");
const APP_JS: &str = include_str!("../web/app.js");

#[derive(Clone)]
pub struct AppState {
    pub status: StatusResponse,
    pub network_script: PathBuf,
    pub library: LibraryBrowser,
    pub mpd: MpdHandle,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.css", get(app_css))
        .route("/app.js", get(app_js))
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
        .route("/api/v1/library/list", get(library))
        .route("/api/v1/library/rescan", post(rescan))
        .route("/api/v1/queue", get(queue).delete(clear_queue))
        .route("/api/v1/queue/play", post(play_queue_item))
        .route("/api/v1/queue/play-next", post(play_queue_item_next))
        .route("/api/v1/queue/remove", post(remove_queue_item))
        .route("/api/v1/queue/move", post(move_queue_item))
        .route("/api/v1/queue/files", post(enqueue_file))
        .route("/api/v1/queue/directories", post(enqueue_directory))
        .with_state(state)
}

async fn index() -> impl IntoResponse {
    static_text("text/html; charset=utf-8", INDEX_HTML)
}

async fn app_css() -> impl IntoResponse {
    static_text("text/css; charset=utf-8", APP_CSS)
}

async fn app_js() -> impl IntoResponse {
    static_text("text/javascript; charset=utf-8", APP_JS)
}

fn static_text(content_type: &'static str, body: &'static str) -> impl IntoResponse {
    ([(CONTENT_TYPE, content_type)], body)
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
    let listing = state.library.list(&query.path)?;
    Ok(Json(listing))
}

async fn rescan(State(state): State<AppState>) -> Result<Json<RescanResponse>, ApiError> {
    let job_id = state.mpd.rescan().await?;
    Ok(Json(RescanResponse { job_id }))
}

async fn queue(State(state): State<AppState>) -> Result<Json<QueueListing>, ApiError> {
    let queue = state.mpd.list_queue().await?;
    Ok(Json(queue))
}

#[derive(Debug, Deserialize)]
struct QueuePathRequest {
    path: String,
}

async fn enqueue_file(
    State(state): State<AppState>,
    Json(req): Json<QueuePathRequest>,
) -> Result<StatusCode, ApiError> {
    let path = state.library.validate_track_path(&req.path)?;
    state.mpd.enqueue(path).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn enqueue_directory(
    State(state): State<AppState>,
    Json(req): Json<QueuePathRequest>,
) -> Result<StatusCode, ApiError> {
    let paths = state.library.audio_files_for_directory(&req.path)?;
    state.mpd.enqueue_paths(paths).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn clear_queue(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state.mpd.clear_queue().await?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Debug, Deserialize)]
struct QueueItemTargetRequest {
    id: Option<u64>,
    position: Option<u32>,
}

async fn play_queue_item(
    State(state): State<AppState>,
    Json(req): Json<QueueItemTargetRequest>,
) -> Result<StatusCode, ApiError> {
    let target = queue_item_target(req.id, req.position)?;
    state.mpd.play_queue_item(target).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn play_queue_item_next(
    State(state): State<AppState>,
    Json(req): Json<QueueItemTargetRequest>,
) -> Result<StatusCode, ApiError> {
    let target = queue_item_target(req.id, req.position)?;
    state.mpd.move_queue_item_after_current(target).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn remove_queue_item(
    State(state): State<AppState>,
    Json(req): Json<QueueItemTargetRequest>,
) -> Result<StatusCode, ApiError> {
    let target = queue_item_target(req.id, req.position)?;
    state.mpd.remove_queue_item(target).await?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Debug, Deserialize)]
struct QueueMoveRequest {
    id: Option<u64>,
    position: Option<u32>,
    to_position: u32,
}

async fn move_queue_item(
    State(state): State<AppState>,
    Json(req): Json<QueueMoveRequest>,
) -> Result<StatusCode, ApiError> {
    let target = queue_item_target(req.id, req.position)?;
    state.mpd.move_queue_item(target, req.to_position).await?;
    Ok(StatusCode::ACCEPTED)
}

fn queue_item_target(id: Option<u64>, position: Option<u32>) -> Result<QueueItemTarget, ApiError> {
    if let Some(id) = id {
        return Ok(QueueItemTarget::Id(id));
    }
    if let Some(position) = position {
        return Ok(QueueItemTarget::Position(position));
    }
    Err(ApiError::bad_request("id or position is required"))
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

impl From<LibraryError> for ApiError {
    fn from(error: LibraryError) -> Self {
        let status = match &error {
            LibraryError::BadPath(_)
            | LibraryError::NotDirectory(_)
            | LibraryError::NotTrack(_)
            | LibraryError::NoSupportedTracks(_)
            | LibraryError::UnsupportedTrack(_) => StatusCode::BAD_REQUEST,
            LibraryError::NotFound(_) => StatusCode::NOT_FOUND,
            LibraryError::Io { .. } => StatusCode::INTERNAL_SERVER_ERROR,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_item_target_prefers_stable_id() {
        assert_eq!(
            queue_item_target(Some(42), Some(3)).expect("target"),
            QueueItemTarget::Id(42)
        );
    }

    #[test]
    fn queue_item_target_accepts_position_fallback() {
        assert_eq!(
            queue_item_target(None, Some(3)).expect("target"),
            QueueItemTarget::Position(3)
        );
    }

    #[test]
    fn queue_item_target_rejects_empty_request() {
        assert!(queue_item_target(None, None).is_err());
    }
}
