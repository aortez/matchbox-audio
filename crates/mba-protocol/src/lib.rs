use serde::{Deserialize, Serialize};

pub const API_VERSION: &str = "v1";
pub const SERVICE_NAME: &str = "matchbox-audio";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResponse {
    pub service: ServiceInfo,
    pub build: BuildInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkInfo>,
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
        }
    }

    pub fn with_network(mut self, network: NetworkInfo) -> Self {
        self.network = Some(network);
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
}
