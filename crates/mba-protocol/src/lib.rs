use serde::{Deserialize, Serialize};

pub const API_VERSION: &str = "v1";
pub const SERVICE_NAME: &str = "matchbox-audio";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResponse {
    pub service: ServiceInfo,
    pub build: BuildInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback: Option<PlaybackInfo>,
}

impl StatusResponse {
    pub fn ready(version: impl Into<String>, git_sha: Option<impl Into<String>>) -> Self {
        Self {
            service: ServiceInfo {
                name: SERVICE_NAME.to_string(),
                state: ServiceState::Ready,
                api_version: API_VERSION.to_string(),
            },
            build: BuildInfo {
                version: version.into(),
                git_sha: git_sha.map(Into::into),
            },
            network: None,
            playback: None,
        }
    }

    pub fn with_network(mut self, network: NetworkInfo) -> Self {
        self.network = Some(network);
        self
    }

    pub fn with_playback(mut self, playback: PlaybackInfo) -> Self {
        self.playback = Some(playback);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub name: String,
    pub state: ServiceState,
    pub api_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Starting,
    Ready,
    Degraded,
}

impl std::fmt::Display for ServiceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Degraded => "degraded",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildInfo {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkMode {
    Car,
    Home,
    Ethernet,
    Unknown,
}

impl NetworkMode {
    pub fn parse(value: &str) -> Self {
        match value {
            "car" => Self::Car,
            "home" => Self::Home,
            "ethernet" => Self::Ethernet,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Car => "car",
            Self::Home => "home",
            Self::Ethernet => "ethernet",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for NetworkMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub mode: NetworkMode,
    pub active_connection: String,
    pub ssid: String,
    pub ip4: String,
    pub hotspot_ssid: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackState {
    Play,
    Pause,
    Stop,
}

impl PlaybackState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Stop => "stop",
        }
    }
}

impl std::fmt::Display for PlaybackState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaybackInfo {
    pub state: PlaybackState,
    pub volume: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<TrackInfo>,
    pub queue_length: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackInfo {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_s: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_s: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryListing {
    pub path: String,
    pub directories: Vec<LibraryDirectory>,
    pub tracks: Vec<LibraryTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryDirectory {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryTrack {
    pub uri: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_s: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RescanResponse {
    pub job_id: u64,
}

pub fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_ready_status_with_snake_case_state() {
        let status = StatusResponse::ready("0.1.0", Some("abc123"));

        let json = serde_json::to_value(&status).expect("status serializes");

        assert_eq!(json["service"]["name"], SERVICE_NAME);
        assert_eq!(json["service"]["state"], "ready");
        assert_eq!(json["service"]["api_version"], API_VERSION);
        assert_eq!(json["build"]["version"], "0.1.0");
        assert_eq!(json["build"]["git_sha"], "abc123");
        assert!(json.get("network").is_none());
    }

    #[test]
    fn serializes_network_status() {
        let status = StatusResponse::ready("0.1.0", None::<String>).with_network(NetworkInfo {
            mode: NetworkMode::Car,
            active_connection: "matchbox-car-hotspot".to_string(),
            ssid: "matchbox-audio".to_string(),
            ip4: "10.42.0.1".to_string(),
            hotspot_ssid: "matchbox-audio".to_string(),
        });

        let json = serde_json::to_value(&status).expect("status serializes");

        assert_eq!(json["network"]["mode"], "car");
        assert_eq!(json["network"]["ip4"], "10.42.0.1");
        assert_eq!(json["network"]["hotspot_ssid"], "matchbox-audio");
    }

    #[test]
    fn parses_unknown_network_mode() {
        assert_eq!(NetworkMode::parse("car"), NetworkMode::Car);
        assert_eq!(NetworkMode::parse("home"), NetworkMode::Home);
        assert_eq!(NetworkMode::parse("ethernet"), NetworkMode::Ethernet);
        assert_eq!(NetworkMode::parse(""), NetworkMode::Unknown);
        assert_eq!(NetworkMode::parse("garbage"), NetworkMode::Unknown);
    }

    #[test]
    fn round_trips_status_response() {
        let status = StatusResponse::ready("0.1.0", None::<String>);

        let json = serde_json::to_string(&status).expect("status serializes");
        let decoded: StatusResponse = serde_json::from_str(&json).expect("status deserializes");

        assert_eq!(decoded, status);
    }

    #[test]
    fn serializes_playback_status_with_track() {
        let status = StatusResponse::ready("0.1.0", None::<String>).with_playback(PlaybackInfo {
            state: PlaybackState::Play,
            volume: 65,
            queue_length: 12,
            track: Some(TrackInfo {
                uri: "_landing-b-test/test-tone.flac".to_string(),
                title: Some("Test Tone".to_string()),
                artist: Some("Matchbox".to_string()),
                album: None,
                duration_s: Some(245),
                elapsed_s: Some(83),
            }),
        });

        let json = serde_json::to_value(&status).expect("status serializes");

        assert_eq!(json["playback"]["state"], "play");
        assert_eq!(json["playback"]["volume"], 65);
        assert_eq!(json["playback"]["queue_length"], 12);
        assert_eq!(json["playback"]["track"]["title"], "Test Tone");
        assert_eq!(json["playback"]["track"]["artist"], "Matchbox");
        assert_eq!(json["playback"]["track"]["duration_s"], 245);
        assert!(json["playback"]["track"].get("album").is_none());
    }

    #[test]
    fn serializes_playback_status_without_track() {
        let status = StatusResponse::ready("0.1.0", None::<String>).with_playback(PlaybackInfo {
            state: PlaybackState::Stop,
            volume: 40,
            queue_length: 0,
            track: None,
        });

        let json = serde_json::to_value(&status).expect("status serializes");

        assert_eq!(json["playback"]["state"], "stop");
        assert_eq!(json["playback"]["volume"], 40);
        assert!(json["playback"].get("track").is_none());
    }

    #[test]
    fn serializes_library_listing() {
        let listing = LibraryListing {
            path: "Pink Floyd".to_string(),
            directories: vec![LibraryDirectory {
                path: "Pink Floyd/Dark Side".to_string(),
                name: "Dark Side".to_string(),
            }],
            tracks: vec![LibraryTrack {
                uri: "Pink Floyd/single.flac".to_string(),
                name: "single.flac".to_string(),
                title: Some("Single".to_string()),
                artist: Some("Pink Floyd".to_string()),
                album: None,
                duration_s: Some(214),
            }],
        };

        let json = serde_json::to_value(&listing).expect("listing serializes");
        assert_eq!(json["path"], "Pink Floyd");
        assert_eq!(json["directories"][0]["name"], "Dark Side");
        assert_eq!(json["tracks"][0]["title"], "Single");
        assert!(json["tracks"][0].get("album").is_none());
    }

    #[test]
    fn round_trips_rescan_response() {
        let response = RescanResponse { job_id: 42 };
        let json = serde_json::to_string(&response).expect("rescan serializes");
        let decoded: RescanResponse = serde_json::from_str(&json).expect("rescan deserializes");
        assert_eq!(decoded, response);
    }

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("Pink Floyd/Dark Side/01 Speak to Me.flac"), "01 Speak to Me.flac");
        assert_eq!(basename("Pink Floyd"), "Pink Floyd");
        assert_eq!(basename(""), "");
    }

    #[test]
    fn round_trips_playback_status() {
        let status = StatusResponse::ready("0.1.0", None::<String>).with_playback(PlaybackInfo {
            state: PlaybackState::Pause,
            volume: 50,
            queue_length: 3,
            track: Some(TrackInfo {
                uri: "songs/foo.flac".to_string(),
                title: Some("Foo".to_string()),
                artist: None,
                album: None,
                duration_s: None,
                elapsed_s: None,
            }),
        });

        let json = serde_json::to_string(&status).expect("status serializes");
        let decoded: StatusResponse = serde_json::from_str(&json).expect("status deserializes");

        assert_eq!(decoded, status);
    }
}
