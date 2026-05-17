use std::{
    error::Error,
    fmt,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use mba_protocol::{
    BuildInfo, NetworkInfo, NetworkMode, PlaybackInfo, PlaybackState, ServiceInfo, ServiceState,
    StatusResponse, TrackInfo, API_VERSION, SERVICE_NAME,
};
use reqwest::{Client, StatusCode, Url};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type PlayerResult<T> = Result<T, PlayerError>;

const DEFAULT_PLAYER_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

pub trait PlayerBackend: Clone + Send + Sync + 'static {
    fn snapshot(&self) -> BoxFuture<'_, PlayerResult<StatusResponse>>;
    fn playback_command(&self, command: PlaybackCommand) -> BoxFuture<'_, PlayerResult<()>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackCommand {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
}

impl PlaybackCommand {
    fn endpoint(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Toggle => "toggle",
            Self::Stop => "stop",
            Self::Next => "next",
            Self::Previous => "previous",
        }
    }
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
pub enum MatchboxPlayerBackend {
    Fake(FakePlayerBackend),
    Http(HttpPlayerBackend),
}

impl MatchboxPlayerBackend {
    pub fn fake_ready() -> Self {
        Self::Fake(FakePlayerBackend::ready())
    }

    pub fn http(base_url: Url) -> PlayerResult<Self> {
        Ok(Self::Http(HttpPlayerBackend::new(base_url)?))
    }
}

impl PlayerBackend for MatchboxPlayerBackend {
    fn snapshot(&self) -> BoxFuture<'_, PlayerResult<StatusResponse>> {
        match self {
            Self::Fake(backend) => backend.snapshot(),
            Self::Http(backend) => backend.snapshot(),
        }
    }

    fn playback_command(&self, command: PlaybackCommand) -> BoxFuture<'_, PlayerResult<()>> {
        match self {
            Self::Fake(backend) => backend.playback_command(command),
            Self::Http(backend) => backend.playback_command(command),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HttpPlayerBackend {
    client: Client,
    base_url: Url,
}

impl HttpPlayerBackend {
    pub fn new(base_url: Url) -> PlayerResult<Self> {
        let client = Client::builder()
            .timeout(DEFAULT_PLAYER_REQUEST_TIMEOUT)
            .build()
            .map_err(|error| {
                PlayerError::Internal(format!("failed to build player HTTP client: {error}"))
            })?;

        Ok(Self { client, base_url })
    }

    fn status_url(&self) -> PlayerResult<Url> {
        self.base_url
            .join("api/v1/status")
            .map_err(|error| PlayerError::Internal(format!("invalid player status URL: {error}")))
    }

    fn playback_url(&self, command: PlaybackCommand) -> PlayerResult<Url> {
        self.base_url
            .join(&format!("api/v1/playback/{}", command.endpoint()))
            .map_err(|error| PlayerError::Internal(format!("invalid player playback URL: {error}")))
    }
}

impl PlayerBackend for HttpPlayerBackend {
    fn snapshot(&self) -> BoxFuture<'_, PlayerResult<StatusResponse>> {
        Box::pin(async move {
            let url = self.status_url()?;
            let response = self.client.get(url.clone()).send().await.map_err(|error| {
                PlayerError::Unavailable(format!("failed to query mba-player at {url}: {error}"))
            })?;

            let status = response.status();
            if !status.is_success() {
                return Err(player_status_error(status, url.as_str()));
            }

            response.json::<StatusResponse>().await.map_err(|error| {
                PlayerError::Internal(format!(
                    "failed to decode mba-player status from {url}: {error}"
                ))
            })
        })
    }

    fn playback_command(&self, command: PlaybackCommand) -> BoxFuture<'_, PlayerResult<()>> {
        Box::pin(async move {
            let url = self.playback_url(command)?;
            let response = self
                .client
                .post(url.clone())
                .send()
                .await
                .map_err(|error| {
                    PlayerError::Unavailable(format!(
                        "failed to query mba-player at {url}: {error}"
                    ))
                })?;

            let status = response.status();
            if !status.is_success() {
                return Err(player_command_error(status, url.as_str()));
            }

            Ok(())
        })
    }
}

fn player_status_error(status: StatusCode, url: &str) -> PlayerError {
    let message = format!("mba-player status request to {url} returned HTTP {status}");
    if status.is_server_error() {
        PlayerError::Unavailable(message)
    } else {
        PlayerError::Internal(message)
    }
}

fn player_command_error(status: StatusCode, url: &str) -> PlayerError {
    let message = format!("mba-player playback request to {url} returned HTTP {status}");
    if status.is_server_error() {
        PlayerError::Unavailable(message)
    } else {
        PlayerError::Internal(message)
    }
}

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

    fn playback_command(&self, command: PlaybackCommand) -> BoxFuture<'_, PlayerResult<()>> {
        Box::pin(async move {
            let mut state = self.state.lock().expect("fake player mutex");
            let snapshot = state.snapshot.as_mut().map_err(|error| error.clone())?;
            let Some(playback) = snapshot.playback.as_mut() else {
                return Ok(());
            };
            match command {
                PlaybackCommand::Play => playback.state = PlaybackState::Play,
                PlaybackCommand::Pause => playback.state = PlaybackState::Pause,
                PlaybackCommand::Toggle => {
                    playback.state = match playback.state {
                        PlaybackState::Play => PlaybackState::Pause,
                        PlaybackState::Pause | PlaybackState::Stop => PlaybackState::Play,
                    };
                }
                PlaybackCommand::Stop => playback.state = PlaybackState::Stop,
                PlaybackCommand::Next | PlaybackCommand::Previous => {}
            }
            Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[tokio::test]
    async fn http_player_backend_fetches_status_snapshot() {
        let expected = sample_snapshot();
        let server = TestHttpServer::responding_with(
            StatusCode::OK,
            serde_json::to_vec(&expected).expect("snapshot serializes"),
        );

        let backend = HttpPlayerBackend::new(server.base_url()).expect("backend builds");
        let snapshot = backend.snapshot().await.expect("snapshot succeeds");

        assert_eq!(snapshot, expected);
        assert_eq!(server.request_path(), "/api/v1/status");
    }

    #[tokio::test]
    async fn http_player_backend_posts_playback_command() {
        let server = TestHttpServer::responding_with(StatusCode::ACCEPTED, b"{}".to_vec());
        let backend = HttpPlayerBackend::new(server.base_url()).expect("backend builds");

        backend
            .playback_command(PlaybackCommand::Next)
            .await
            .expect("command succeeds");

        assert_eq!(server.request_path(), "/api/v1/playback/next");
    }

    #[tokio::test]
    async fn fake_player_backend_updates_playback_state() {
        let backend = FakePlayerBackend::ready();

        backend
            .playback_command(PlaybackCommand::Pause)
            .await
            .expect("pause succeeds");

        let snapshot = backend.snapshot().await.expect("snapshot succeeds");
        assert_eq!(
            snapshot.playback.expect("playback").state,
            PlaybackState::Pause
        );
    }

    #[tokio::test]
    async fn http_player_backend_reports_player_unavailable_on_server_error() {
        let server =
            TestHttpServer::responding_with(StatusCode::SERVICE_UNAVAILABLE, b"not ready".to_vec());
        let backend = HttpPlayerBackend::new(server.base_url()).expect("backend builds");

        let error = backend.snapshot().await.expect_err("server error fails");

        assert!(matches!(error, PlayerError::Unavailable(_)));
        assert!(error.to_string().contains("HTTP 503 Service Unavailable"));
    }

    #[tokio::test]
    async fn http_player_backend_reports_internal_error_on_bad_json() {
        let server = TestHttpServer::responding_with(StatusCode::OK, b"not json".to_vec());
        let backend = HttpPlayerBackend::new(server.base_url()).expect("backend builds");

        let error = backend.snapshot().await.expect_err("bad json fails");

        assert!(matches!(error, PlayerError::Internal(_)));
        assert!(error.to_string().contains("failed to decode"));
    }

    struct TestHttpServer {
        base_url: Url,
        join: thread::JoinHandle<String>,
    }

    impl TestHttpServer {
        fn responding_with(status: StatusCode, body: Vec<u8>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("test listener binds");
            let addr = listener.local_addr().expect("test listener address");
            let join = thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("test connection accepted");
                let mut request = Vec::new();
                let mut buffer = [0u8; 1024];
                loop {
                    let n = stream.read(&mut buffer).expect("test request read");
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..n]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }

                let status_line = format!(
                    "HTTP/1.1 {} {}\r\n",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("Unknown")
                );
                stream
                    .write_all(status_line.as_bytes())
                    .expect("status line written");
                write!(
                    stream,
                    "content-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                )
                .expect("headers written");
                stream.write_all(&body).expect("body written");

                let request = String::from_utf8_lossy(&request);
                request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("")
                    .to_string()
            });

            Self {
                base_url: Url::parse(&format!("http://{addr}/")).expect("test URL parses"),
                join,
            }
        }

        fn base_url(&self) -> Url {
            self.base_url.clone()
        }

        fn request_path(self) -> String {
            self.join.join().expect("test server joined")
        }
    }
}
