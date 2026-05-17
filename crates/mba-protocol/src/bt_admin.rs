use serde::{Deserialize, Serialize};

pub const DEFAULT_BT_CONTROL_SOCKET: &str = "/run/mba-bt/control.sock";
pub const BT_CONTROL_METHOD_STATUS: &str = "bt.status";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BtControlRequest {
    pub method: String,
}

impl BtControlRequest {
    pub fn status() -> Self {
        Self {
            method: BT_CONTROL_METHOD_STATUS.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BtControlResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<BtStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<BtControlError>,
}

impl BtControlResponse {
    pub fn ok_status(status: BtStatus) -> Self {
        Self {
            ok: true,
            status: Some(status),
            error: None,
        }
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            status: None,
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
    pub adapter: Option<BtAdapterStatus>,
    pub advertising: bool,
    pub service_uuid: String,
    pub pairing_state: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_request_uses_stable_method_name() {
        let request = BtControlRequest::status();

        assert_eq!(request.method, "bt.status");
    }

    #[test]
    fn status_response_round_trips() {
        let status = BtStatus {
            service: "matchbox-audio".to_string(),
            transport: "mba-bt-ble-local".to_string(),
            device_name: "Matchbox Audio".to_string(),
            adapter: Some(BtAdapterStatus {
                name: "hci0".to_string(),
                address: "88:A2:9E:B1:87:91".to_string(),
            }),
            advertising: true,
            service_uuid: "1cef04f1-966e-43ad-860f-086db4f277d6".to_string(),
            pairing_state: "local".to_string(),
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
}
