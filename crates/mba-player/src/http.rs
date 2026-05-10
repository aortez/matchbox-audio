use std::{
    path::{Component, Path, PathBuf},
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

use crate::mpd::{MpdError, MpdHandle, QueuePlaybackTarget};

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_CSS: &str = include_str!("../web/app.css");
const APP_JS: &str = include_str!("../web/app.js");

#[derive(Clone)]
pub struct AppState {
    pub status: StatusResponse,
    pub network_script: PathBuf,
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
        .route("/api/v1/library/rescan", post(rescan))
        .route("/api/v1/queue", get(queue).delete(clear_queue))
        .route("/api/v1/queue/play", post(play_queue_item))
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
    let listing = state.mpd.list_library(query.path).await?;
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
    let path = validate_queue_path(req.path)?;
    state.mpd.enqueue(path).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn enqueue_directory(
    State(state): State<AppState>,
    Json(req): Json<QueuePathRequest>,
) -> Result<StatusCode, ApiError> {
    let path = validate_queue_path(req.path)?;
    state.mpd.enqueue_directory(path).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn clear_queue(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state.mpd.clear_queue().await?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Debug, Deserialize)]
struct QueuePlayRequest {
    id: Option<u64>,
    position: Option<u32>,
}

async fn play_queue_item(
    State(state): State<AppState>,
    Json(req): Json<QueuePlayRequest>,
) -> Result<StatusCode, ApiError> {
    let target = queue_play_target(req)?;
    state.mpd.play_queue_item(target).await?;
    Ok(StatusCode::ACCEPTED)
}

fn queue_play_target(req: QueuePlayRequest) -> Result<QueuePlaybackTarget, ApiError> {
    if let Some(id) = req.id {
        return Ok(QueuePlaybackTarget::Id(id));
    }
    if let Some(position) = req.position {
        return Ok(QueuePlaybackTarget::Position(position));
    }
    Err(ApiError::bad_request("id or position is required"))
}

fn validate_queue_path(path: String) -> Result<String, ApiError> {
    let path = path.trim().to_string();
    if path.is_empty() {
        return Err(ApiError::bad_request("path must not be empty"));
    }
    if path.contains('\0') {
        return Err(ApiError::bad_request("path must not contain NUL bytes"));
    }
    if path.contains("://") || path.starts_with("file:") {
        return Err(ApiError::bad_request("path must be a library path"));
    }
    let library_path = Path::new(&path);
    if library_path.is_absolute() {
        return Err(ApiError::bad_request("path must be relative"));
    }
    for component in library_path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => return Err(ApiError::bad_request("path must not contain .")),
            Component::ParentDir => return Err(ApiError::bad_request("path must not contain ..")),
            Component::RootDir | Component::Prefix(_) => {
                return Err(ApiError::bad_request("path must be relative"));
            }
        }
    }
    Ok(path)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_queue_path_accepts_relative_music_paths() {
        assert_eq!(
            validate_queue_path("Air/Moon Safari/01 La femme d'argent.ogg".to_string())
                .expect("valid path"),
            "Air/Moon Safari/01 La femme d'argent.ogg"
        );
    }

    #[test]
    fn validate_queue_path_rejects_paths_outside_library() {
        for path in [
            "",
            "/data/music/song.flac",
            "../song.flac",
            "Air/../song.flac",
            "./song.flac",
            "http://example.test/stream.mp3",
            "file:///data/music/song.flac",
        ] {
            assert!(
                validate_queue_path(path.to_string()).is_err(),
                "expected {path:?} to be rejected"
            );
        }
    }

    #[test]
    fn queue_play_target_prefers_stable_id() {
        assert_eq!(
            queue_play_target(QueuePlayRequest {
                id: Some(42),
                position: Some(3),
            })
            .expect("target"),
            QueuePlaybackTarget::Id(42)
        );
    }

    #[test]
    fn queue_play_target_accepts_position_fallback() {
        assert_eq!(
            queue_play_target(QueuePlayRequest {
                id: None,
                position: Some(3),
            })
            .expect("target"),
            QueuePlaybackTarget::Position(3)
        );
    }

    #[test]
    fn queue_play_target_rejects_empty_request() {
        assert!(queue_play_target(QueuePlayRequest {
            id: None,
            position: None,
        })
        .is_err());
    }
}
