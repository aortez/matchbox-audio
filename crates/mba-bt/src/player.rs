use std::{
    error::Error,
    fmt,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

use mba_protocol::{
    BuildInfo, NetworkInfo, NetworkMode, PlaybackInfo, PlaybackState, ServiceInfo, ServiceState,
    StatusResponse, TrackInfo, API_VERSION, SERVICE_NAME,
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type PlayerResult<T> = Result<T, PlayerError>;

pub trait PlayerBackend: Clone + Send + Sync + 'static {
    fn snapshot(&self) -> BoxFuture<'_, PlayerResult<StatusResponse>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerError {
    Unavailable(String),
    Internal(String),
}

impl fmt::Display for PlayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) => write!(f, "player unavailable: {message}"),
            Self::Internal(message) => write!(f, "player error: {message}"),
        }
    }
}

impl Error for PlayerError {}

#[derive(Debug, Clone)]
pub struct FakePlayerBackend {
    state: Arc<Mutex<FakePlayerState>>,
}

impl FakePlayerBackend {
    pub fn ready() -> Self {
        Self {
            state: Arc::new(Mutex::new(FakePlayerState {
                snapshot: Ok(sample_snapshot()),
            })),
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakePlayerState {
                snapshot: Err(PlayerError::Unavailable(message.into())),
            })),
        }
    }

    pub fn set_snapshot(&self, snapshot: StatusResponse) {
        self.state.lock().expect("fake player mutex").snapshot = Ok(snapshot);
    }
}

impl PlayerBackend for FakePlayerBackend {
    fn snapshot(&self) -> BoxFuture<'_, PlayerResult<StatusResponse>> {
        Box::pin(async move {
            self.state
                .lock()
                .expect("fake player mutex")
                .snapshot
                .clone()
        })
    }
}

#[derive(Debug, Clone)]
struct FakePlayerState {
    snapshot: PlayerResult<StatusResponse>,
}

pub fn sample_snapshot() -> StatusResponse {
    StatusResponse {
        service: ServiceInfo {
            name: SERVICE_NAME.to_string(),
            state: ServiceState::Ready,
            api_version: API_VERSION.to_string(),
        },
        build: BuildInfo {
            version: "0.1.0".to_string(),
            git_sha: Some("fake-player".to_string()),
        },
        network: Some(NetworkInfo {
            mode: NetworkMode::Car,
            active_connection: "matchbox-car-hotspot".to_string(),
            ssid: "matchbox-audio".to_string(),
            ip4: "10.42.0.1".to_string(),
            hotspot_ssid: "matchbox-audio".to_string(),
        }),
        playback: Some(PlaybackInfo {
            state: PlaybackState::Play,
            volume: 65,
            queue_position: Some(0),
            queue_id: Some(12),
            track: Some(TrackInfo {
                uri: "Pink Floyd/single.flac".to_string(),
                title: Some("Single".to_string()),
                artist: Some("Pink Floyd".to_string()),
                album: None,
                duration_s: Some(214),
                elapsed_s: Some(12),
            }),
            queue_length: 1,
        }),
    }
}
