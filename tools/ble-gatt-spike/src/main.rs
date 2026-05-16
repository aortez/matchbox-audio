use std::sync::Arc;

use anyhow::Context;
use bluer::{
    adv::Advertisement,
    gatt::{
        local::{
            characteristic_control, Application, Characteristic, CharacteristicControl,
            CharacteristicControlEvent, CharacteristicNotify, CharacteristicNotifyMethod,
            CharacteristicRead, CharacteristicWrite, CharacteristicWriteMethod, Service,
        },
        CharacteristicWriter,
    },
    Session, Uuid,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

const SERVICE_UUID: Uuid = Uuid::from_u128(0x1cef04f1_966e_43ad_860f_086db4f277d6);
const STATUS_UUID: Uuid = Uuid::from_u128(0xbd539314_4637_416b_a3b5_804fecd5b792);
const RX_UUID: Uuid = Uuid::from_u128(0xfbf39e22_bb07_49bf_bfa0_3dbdfc47769b);
const TX_UUID: Uuid = Uuid::from_u128(0xfcc9055c_34e3_46d9_a010_bd8a4f180b0c);

const DEVICE_NAME: &str = "Matchbox Audio";
const CHUNK_MAGIC: &[u8; 2] = b"MB";
const CHUNK_VERSION: u8 = 1;
const FLAG_FIRST_CHUNK: u8 = 0x01;
const FLAG_LAST_CHUNK: u8 = 0x02;
const CHUNK_HEADER_BYTES: usize = 16;
const MAX_MESSAGE_BYTES: usize = 16 * 1024;
const TARGET_GATT_VALUE_BYTES: usize = 244;
const TARGET_CHUNK_PAYLOAD_BYTES: usize = TARGET_GATT_VALUE_BYTES - CHUNK_HEADER_BYTES;

#[derive(Default)]
struct SpikeState {
    tx_writer: Option<CharacteristicWriter>,
    rx_count: u64,
    partial_rx: Option<PartialMessage>,
}

struct PartialMessage {
    message_id: u32,
    next_chunk_index: u16,
    chunk_count: u16,
    total_message_len: usize,
    payload: Vec<u8>,
}

struct Chunk {
    flags: u8,
    message_id: u32,
    chunk_index: u16,
    chunk_count: u16,
    total_message_len: usize,
    payload_fragment: Vec<u8>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ble_gatt_spike=debug,info".into()),
        )
        .init();

    info!("starting Matchbox BLE GATT spike");

    let state = Arc::new(RwLock::new(SpikeState::default()));
    let session = Session::new()
        .await
        .context("failed to connect to BlueZ over D-Bus")?;
    let adapter = session
        .default_adapter()
        .await
        .context("failed to find default Bluetooth adapter")?;

    adapter
        .set_powered(true)
        .await
        .context("failed to power Bluetooth adapter")?;

    if let Err(error) = adapter.set_alias(DEVICE_NAME.to_string()).await {
        warn!(%error, "failed to set adapter alias; continuing");
    }

    let address = adapter.address().await?;
    info!(adapter = %adapter.name(), %address, "using Bluetooth adapter");

    let (app, tx_control) = build_gatt_application(Arc::clone(&state));
    let _app_handle = adapter
        .serve_gatt_application(app)
        .await
        .context("failed to register GATT application")?;
    info!(service_uuid = %SERVICE_UUID, "registered Matchbox GATT service");

    tokio::spawn(handle_tx_subscriptions(Arc::clone(&state), tx_control));

    let advertisement = Advertisement {
        service_uuids: vec![SERVICE_UUID].into_iter().collect(),
        local_name: Some(DEVICE_NAME.to_string()),
        discoverable: Some(true),
        ..Default::default()
    };
    let _adv_handle = adapter
        .advertise(advertisement)
        .await
        .context("failed to start BLE advertisement")?;
    info!(name = DEVICE_NAME, "advertising started");

    info!("press Ctrl-C to stop the BLE spike");
    tokio::signal::ctrl_c()
        .await
        .context("failed waiting for Ctrl-C")?;

    info!("stopping BLE spike");
    Ok(())
}

fn build_gatt_application(state: Arc<RwLock<SpikeState>>) -> (Application, CharacteristicControl) {
    let status_read = CharacteristicRead {
        read: true,
        fun: Box::new(move |_req| Box::pin(async move { Ok(status_payload()) })),
        ..Default::default()
    };

    let rx_state = Arc::clone(&state);
    let rx_write = CharacteristicWrite {
        write: true,
        method: CharacteristicWriteMethod::Fun(Box::new(move |new_value, _req| {
            let rx_state = Arc::clone(&rx_state);
            Box::pin(async move {
                handle_rx_chunk(rx_state, new_value).await;
                Ok(())
            })
        })),
        ..Default::default()
    };

    let (tx_control, tx_handle) = characteristic_control();
    let tx_notify = CharacteristicNotify {
        notify: true,
        method: CharacteristicNotifyMethod::Io,
        ..Default::default()
    };

    let app = Application {
        services: vec![Service {
            uuid: SERVICE_UUID,
            primary: true,
            characteristics: vec![
                Characteristic {
                    uuid: STATUS_UUID,
                    read: Some(status_read),
                    ..Default::default()
                },
                Characteristic {
                    uuid: RX_UUID,
                    write: Some(rx_write),
                    ..Default::default()
                },
                Characteristic {
                    uuid: TX_UUID,
                    control_handle: tx_handle,
                    notify: Some(tx_notify),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };

    (app, tx_control)
}

async fn handle_tx_subscriptions(
    state: Arc<RwLock<SpikeState>>,
    mut tx_control: CharacteristicControl,
) {
    while let Some(event) = tx_control.next().await {
        match event {
            CharacteristicControlEvent::Notify(writer) => {
                info!("client subscribed to TX notifications");
                let mut state = state.write().await;
                state.tx_writer = Some(writer);
            }
            CharacteristicControlEvent::Write(_) => {
                debug!("ignoring write event on TX characteristic");
            }
        }
    }
}

async fn handle_rx_chunk(state: Arc<RwLock<SpikeState>>, chunk: Vec<u8>) {
    info!(bytes = chunk.len(), "received RX chunk");
    debug!(chunk = ?chunk, "RX chunk bytes");

    let mut state = state.write().await;
    state.rx_count += 1;

    let chunk = match parse_chunk(&chunk) {
        Ok(chunk) => chunk,
        Err(error) => {
            warn!(%error, "rejected RX chunk");
            state.partial_rx = None;
            return;
        }
    };

    let Some(message) = reassemble_chunk(&mut state.partial_rx, chunk) else {
        return;
    };

    info!(bytes = message.len(), "reassembled RX message");

    let response = handle_message(&message);
    let response_payload = response.to_string().into_bytes();
    let response_message_id = 1;

    let Some(writer) = state.tx_writer.as_mut() else {
        warn!("no TX subscriber; cannot send response");
        return;
    };

    for chunk in encode_chunks(response_message_id, &response_payload) {
        if let Err(error) = writer.send(&chunk).await {
            warn!(%error, "failed to send TX chunk");
            return;
        }
        info!(bytes = chunk.len(), "sent TX chunk");
    }
}

fn parse_chunk(bytes: &[u8]) -> anyhow::Result<Chunk> {
    if bytes.len() < CHUNK_HEADER_BYTES {
        anyhow::bail!("chunk too short: {} bytes", bytes.len());
    }
    if &bytes[0..2] != CHUNK_MAGIC {
        anyhow::bail!("bad chunk magic");
    }
    if bytes[2] != CHUNK_VERSION {
        anyhow::bail!("unsupported chunk version {}", bytes[2]);
    }

    let flags = bytes[3];
    let message_id = u32::from_le_bytes(bytes[4..8].try_into()?);
    let chunk_index = u16::from_le_bytes(bytes[8..10].try_into()?);
    let chunk_count = u16::from_le_bytes(bytes[10..12].try_into()?);
    let total_message_len = u32::from_le_bytes(bytes[12..16].try_into()?) as usize;
    let payload_fragment = bytes[16..].to_vec();

    if chunk_count == 0 {
        anyhow::bail!("chunk_count must be nonzero");
    }
    if chunk_index >= chunk_count {
        anyhow::bail!("chunk_index {} >= chunk_count {}", chunk_index, chunk_count);
    }
    if total_message_len > MAX_MESSAGE_BYTES {
        anyhow::bail!(
            "message too large: {} > {}",
            total_message_len,
            MAX_MESSAGE_BYTES
        );
    }
    if chunk_index == 0 && flags & FLAG_FIRST_CHUNK == 0 {
        anyhow::bail!("first chunk flag missing");
    }
    if chunk_index + 1 == chunk_count && flags & FLAG_LAST_CHUNK == 0 {
        anyhow::bail!("last chunk flag missing");
    }

    Ok(Chunk {
        flags,
        message_id,
        chunk_index,
        chunk_count,
        total_message_len,
        payload_fragment,
    })
}

fn reassemble_chunk(partial: &mut Option<PartialMessage>, chunk: Chunk) -> Option<Vec<u8>> {
    if chunk.chunk_index == 0 {
        *partial = Some(PartialMessage {
            message_id: chunk.message_id,
            next_chunk_index: 0,
            chunk_count: chunk.chunk_count,
            total_message_len: chunk.total_message_len,
            payload: Vec::with_capacity(chunk.total_message_len),
        });
    }

    let current = match partial.as_mut() {
        Some(current) => current,
        None => {
            warn!("received non-first chunk without active message");
            return None;
        }
    };

    if current.message_id != chunk.message_id
        || current.chunk_count != chunk.chunk_count
        || current.total_message_len != chunk.total_message_len
        || current.next_chunk_index != chunk.chunk_index
    {
        warn!("received out-of-order or mismatched chunk; dropping partial message");
        *partial = None;
        return None;
    }

    current.payload.extend_from_slice(&chunk.payload_fragment);
    current.next_chunk_index += 1;

    if current.payload.len() > current.total_message_len {
        warn!("partial message exceeded advertised length");
        *partial = None;
        return None;
    }

    if chunk.flags & FLAG_LAST_CHUNK == 0 {
        return None;
    }

    let completed = partial.take().expect("partial message exists");
    if completed.payload.len() != completed.total_message_len {
        warn!(
            actual = completed.payload.len(),
            expected = completed.total_message_len,
            "completed message length mismatch"
        );
        return None;
    }

    Some(completed.payload)
}

fn handle_message(message: &[u8]) -> Value {
    let request: Value = match serde_json::from_slice(message) {
        Ok(request) => request,
        Err(error) => {
            return json!({
                "type": "response",
                "id": null,
                "ok": false,
                "error": {
                    "code": "bad_request",
                    "message": format!("invalid JSON: {error}")
                }
            });
        }
    };

    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();

    info!(%method, "handling request");

    if method == "system.hello" {
        return json!({
            "type": "response",
            "id": id,
            "ok": true,
            "result": {
                "selected_protocol_version": 1,
                "device_name": DEVICE_NAME,
                "build_version": "ble-gatt-spike",
                "capabilities": ["system.hello", "ble.chunk.v1"],
                "transport": {
                    "kind": "ble-gatt",
                    "chunk_protocol_version": CHUNK_VERSION,
                    "max_message_bytes": MAX_MESSAGE_BYTES,
                    "target_response_bytes": 8 * 1024,
                    "target_gatt_value_bytes": TARGET_GATT_VALUE_BYTES,
                    "chunk_header_bytes": CHUNK_HEADER_BYTES,
                    "target_chunk_payload_bytes": TARGET_CHUNK_PAYLOAD_BYTES,
                    "one_in_flight_per_direction": true
                },
                "pairing": {
                    "state": "spike",
                    "trusted": true
                }
            }
        });
    }

    json!({
        "type": "response",
        "id": id,
        "ok": false,
        "error": {
            "code": "unsupported_method",
            "message": format!("unsupported method: {method}")
        }
    })
}

fn encode_chunks(message_id: u32, payload: &[u8]) -> Vec<Vec<u8>> {
    let chunk_count = payload.len().div_ceil(TARGET_CHUNK_PAYLOAD_BYTES).max(1);
    assert!(chunk_count <= u16::MAX as usize);
    assert!(payload.len() <= u32::MAX as usize);

    let mut chunks = Vec::with_capacity(chunk_count);
    for chunk_index in 0..chunk_count {
        let start = chunk_index * TARGET_CHUNK_PAYLOAD_BYTES;
        let end = usize::min(start + TARGET_CHUNK_PAYLOAD_BYTES, payload.len());
        let mut flags = 0u8;
        if chunk_index == 0 {
            flags |= FLAG_FIRST_CHUNK;
        }
        if chunk_index + 1 == chunk_count {
            flags |= FLAG_LAST_CHUNK;
        }

        let mut chunk = Vec::with_capacity(CHUNK_HEADER_BYTES + end.saturating_sub(start));
        chunk.extend_from_slice(CHUNK_MAGIC);
        chunk.push(CHUNK_VERSION);
        chunk.push(flags);
        chunk.extend_from_slice(&message_id.to_le_bytes());
        chunk.extend_from_slice(&(chunk_index as u16).to_le_bytes());
        chunk.extend_from_slice(&(chunk_count as u16).to_le_bytes());
        chunk.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        chunk.extend_from_slice(&payload[start..end]);
        chunks.push(chunk);
    }

    chunks
}

fn status_payload() -> Vec<u8> {
    json!({
        "service": "matchbox-audio",
        "transport": "ble-gatt-spike",
        "protocol_versions": [1],
        "chunk_protocol_version": 1,
        "max_message_bytes": MAX_MESSAGE_BYTES,
        "target_response_bytes": 8 * 1024,
        "target_gatt_value_bytes": TARGET_GATT_VALUE_BYTES,
        "chunk_header_bytes": CHUNK_HEADER_BYTES,
        "target_chunk_payload_bytes": TARGET_CHUNK_PAYLOAD_BYTES,
        "default_page_limit": 50,
        "max_page_limit": 100,
        "one_in_flight_per_direction": true,
        "pairing_state": "spike"
    })
    .to_string()
    .into_bytes()
}
