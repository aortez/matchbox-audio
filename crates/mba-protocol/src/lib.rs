use serde::{Deserialize, Serialize};

pub const API_VERSION: &str = "v1";
pub const SERVICE_NAME: &str = "matchbox-audio";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResponse {
    pub service: ServiceInfo,
    pub build: BuildInfo,
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
        }
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
    }

    #[test]
    fn round_trips_status_response() {
        let status = StatusResponse::ready("0.1.0", None::<String>);

        let json = serde_json::to_string(&status).expect("status serializes");
        let decoded: StatusResponse = serde_json::from_str(&json).expect("status deserializes");

        assert_eq!(decoded, status);
    }
}
