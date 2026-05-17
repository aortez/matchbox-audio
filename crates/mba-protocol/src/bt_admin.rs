use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const DEFAULT_BT_CONTROL_SOCKET: &str = "/run/mba-bt/control.sock";
pub const DEFAULT_BT_STATE_DIR: &str = "/data/matchbox-audio/bt";
pub const DEFAULT_BT_PAIRING_TIMEOUT_SECONDS: u64 = 120;
pub const MAX_BT_PAIRING_TIMEOUT_SECONDS: u64 = 600;
pub const BT_CONTROL_METHOD_STATUS: &str = "bt.status";
pub const BT_CONTROL_METHOD_PAIRING_START: &str = "bt.pairing.start";
pub const BT_CONTROL_METHOD_PAIRING_STOP: &str = "bt.pairing.stop";
pub const BT_CONTROL_METHOD_CLIENTS: &str = "bt.clients";
pub const BT_CONTROL_METHOD_FORGET_CLIENT: &str = "bt.client.forget";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BtControlRequest {
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl BtControlRequest {
    pub fn status() -> Self {
        Self {
            method: BT_CONTROL_METHOD_STATUS.to_string(),
            params: None,
        }
    }

    pub fn pairing_start(timeout_seconds: u64) -> Self {
        Self {
            method: BT_CONTROL_METHOD_PAIRING_START.to_string(),
            params: Some(json!({
                "timeout_seconds": timeout_seconds,
            })),
        }
    }

    pub fn pairing_stop() -> Self {
        Self {
            method: BT_CONTROL_METHOD_PAIRING_STOP.to_string(),
            params: None,
        }
    }

    pub fn clients() -> Self {
        Self {
            method: BT_CONTROL_METHOD_CLIENTS.to_string(),
            params: None,
        }
    }

    pub fn forget_client(client_id: impl Into<String>) -> Self {
        Self {
            method: BT_CONTROL_METHOD_FORGET_CLIENT.to_string(),
            params: Some(json!({
                "client_id": client_id.into(),
            })),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BtControlResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<BtStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clients: Option<Vec<BtClientRecord>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<BtControlError>,
}

impl BtControlResponse {
    pub fn ok_status(status: BtStatus) -> Self {
        Self {
            ok: true,
            status: Some(status),
            clients: None,
            error: None,
        }
    }

    pub fn ok_clients(clients: Vec<BtClientRecord>) -> Self {
        Self {
            ok: true,
            status: None,
            clients: Some(clients),
            error: None,
        }
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            status: None,
            clients: None,
            error: Some(BtControlError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BtControlError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BtStatus {
    pub service: String,
    pub transport: String,
    pub device_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_dir: Option<String>,
    #[serde(default)]
    pub trusted_clients: usize,
    pub adapter: Option<BtAdapterStatus>,
    pub advertising: bool,
    pub service_uuid: String,
    pub pairing_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pairing_remaining_seconds: Option<u64>,
    pub busy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_client: Option<BtActiveClientStatus>,
    pub rx_chunk_writes: u64,
    pub tx_chunks_sent: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BtAdapterStatus {
    pub name: String,
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BtActiveClientStatus {
    pub address: String,
    pub adapter: String,
    pub mtu: usize,
    pub session_token: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BtClientRecord {
    pub schema_version: u32,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub trusted: bool,
    pub created_unix_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_unix_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_ble_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_request_uses_stable_method_name() {
        let request = BtControlRequest::status();

        assert_eq!(request.method, "bt.status");
        assert_eq!(request.params, None);
    }

    #[test]
    fn pairing_start_request_includes_timeout() {
        let request = BtControlRequest::pairing_start(120);

        assert_eq!(request.method, "bt.pairing.start");
        assert_eq!(request.params.expect("params")["timeout_seconds"], 120);
    }

    #[test]
    fn forget_client_request_includes_client_id() {
        let request = BtControlRequest::forget_client("phone-1");

        assert_eq!(request.method, "bt.client.forget");
        assert_eq!(request.params.expect("params")["client_id"], "phone-1");
    }

    #[test]
    fn status_response_round_trips() {
        let status = BtStatus {
            service: "matchbox-audio".to_string(),
            transport: "mba-bt-ble-local".to_string(),
            device_name: "Matchbox Audio".to_string(),
            state_dir: Some("/data/matchbox-audio/bt".to_string()),
            trusted_clients: 0,
            adapter: Some(BtAdapterStatus {
                name: "hci0".to_string(),
                address: "88:A2:9E:B1:87:91".to_string(),
            }),
            advertising: true,
            service_uuid: "1cef04f1-966e-43ad-860f-086db4f277d6".to_string(),
            pairing_state: "open".to_string(),
            pairing_remaining_seconds: Some(120),
            busy: true,
            active_client: Some(BtActiveClientStatus {
                address: "6A:2E:A9:9C:0A:81".to_string(),
                adapter: "hci0".to_string(),
                mtu: 512,
                session_token: 7,
            }),
            rx_chunk_writes: 2,
            tx_chunks_sent: 6,
        };
        let response = BtControlResponse::ok_status(status.clone());

        let json = serde_json::to_string(&response).expect("response serializes");
        let decoded: BtControlResponse =
            serde_json::from_str(&json).expect("response deserializes");

        assert_eq!(decoded, response);
        assert_eq!(decoded.status, Some(status));
    }

    #[test]
    fn clients_response_round_trips() {
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
        let response = BtControlResponse::ok_clients(clients.clone());

        let json = serde_json::to_string(&response).expect("response serializes");
        let decoded: BtControlResponse =
            serde_json::from_str(&json).expect("response deserializes");

        assert_eq!(decoded, response);
        assert_eq!(decoded.clients, Some(clients));
    }
}
