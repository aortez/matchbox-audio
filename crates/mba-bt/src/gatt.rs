use std::{path::PathBuf, sync::Arc};

use anyhow::Context;
use bluer::{
    adv::Advertisement,
    gatt::{
        local::{
            characteristic_control, Application, Characteristic, CharacteristicControl,
            CharacteristicControlEvent, CharacteristicNotify, CharacteristicNotifyMethod,
            CharacteristicRead, CharacteristicWrite, CharacteristicWriteMethod, ReqError, Service,
        },
        CharacteristicWriter,
    },
    Address, Session, Uuid,
};
use futures_util::{FutureExt, StreamExt};
use mba_protocol::{
    ble, BtActiveClientStatus, BtAdapterStatus, BtControlRequest, BtControlResponse, BtStatus,
    ErrorCode, ProtocolError, ProtocolMessage, APP_PROTOCOL_VERSION, BT_CONTROL_METHOD_STATUS,
    DEFAULT_BT_CONTROL_SOCKET, DEFAULT_PAGE_LIMIT, DEVICE_DISPLAY_NAME, MAX_MESSAGE_BYTES,
    MAX_PAGE_LIMIT, SERVICE_NAME, TARGET_RESPONSE_BYTES,
};
use serde_json::json;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

use crate::{
    bad_request_for_transport_error, ActiveSession, BleTransportError, InMemoryBleTransport,
    PlayerBackend, RequestRouter, SessionGate,
};
use crate::{start_control_socket, ControlHandler};

const SERVICE_UUID: Uuid = Uuid::from_u128(0x1cef04f1_966e_43ad_860f_086db4f277d6);
const STATUS_UUID: Uuid = Uuid::from_u128(0xbd539314_4637_416b_a3b5_804fecd5b792);
const RX_UUID: Uuid = Uuid::from_u128(0xfbf39e22_bb07_49bf_bfa0_3dbdfc47769b);
const TX_UUID: Uuid = Uuid::from_u128(0xfcc9055c_34e3_46d9_a010_bd8a4f180b0c);

#[derive(Debug, Clone)]
pub struct BleGattOptions {
    pub device_name: String,
    pub pairing_state: String,
    pub control_socket: Option<PathBuf>,
}

impl Default for BleGattOptions {
    fn default() -> Self {
        Self {
            device_name: DEVICE_DISPLAY_NAME.to_string(),
            pairing_state: "local".to_string(),
            control_socket: Some(DEFAULT_BT_CONTROL_SOCKET.into()),
        }
    }
}

struct BleGattState<P> {
    router: RequestRouter<P>,
    session_gate: SessionGate,
    transport: InMemoryBleTransport,
    active_session: Option<BleActiveSession>,
    next_session_token: u64,
    rx_count: u64,
    tx_sent_count: u64,
}

impl<P> BleGattState<P>
where
    P: PlayerBackend,
{
    fn new(router: RequestRouter<P>) -> Self {
        Self {
            router,
            session_gate: SessionGate::new(),
            transport: InMemoryBleTransport::new(),
            active_session: None,
            next_session_token: 1,
            rx_count: 0,
            tx_sent_count: 0,
        }
    }

    fn status_snapshot(&self) -> BleGattStatusSnapshot {
        BleGattStatusSnapshot {
            busy: self.active_session.is_some(),
            active_client: self
                .active_session
                .as_ref()
                .map(BleActiveClientSnapshot::from),
            rx_count: self.rx_count,
            tx_sent_count: self.tx_sent_count,
        }
    }

    fn start_tx_session(
        &mut self,
        adapter_name: String,
        address: Address,
        mtu: usize,
    ) -> Result<(u64, mpsc::UnboundedReceiver<Vec<Vec<u8>>>), ProtocolError> {
        if let Some(active) = self.active_session.as_ref() {
            if active.address != address {
                return Err(ProtocolError::new(
                    ErrorCode::Busy,
                    format!("client {} is already connected", active.address),
                ));
            }
        }

        let (tx_sender, tx_receiver) = mpsc::unbounded_channel();
        let session_token = self.allocate_session_token();
        self.transport = InMemoryBleTransport::new();

        if let Some(active) = self.active_session.as_mut() {
            warn!(
                client_address = %address,
                old_session_token = active.session_token,
                session_token,
                mtu,
                "client refreshed TX notification subscription"
            );
            active.adapter_name = adapter_name;
            active.mtu = mtu;
            active.session_token = session_token;
            active.tx_sender = tx_sender;
            return Ok((session_token, tx_receiver));
        }

        let client_id = client_id_for_address(address);
        let gate_session = self.session_gate.connect(client_id)?;
        self.active_session = Some(BleActiveSession {
            address,
            adapter_name,
            mtu,
            session_token,
            tx_sender,
            _gate_session: gate_session,
        });
        Ok((session_token, tx_receiver))
    }

    fn finish_tx_session(&mut self, address: Address, session_token: u64) -> bool {
        let should_clear = self.active_session.as_ref().is_some_and(|active| {
            active.address == address && active.session_token == session_token
        });

        if should_clear {
            self.active_session = None;
            self.transport.reset();
            return true;
        }

        false
    }

    fn active_client_allows(&self, address: Address) -> bool {
        self.active_session
            .as_ref()
            .is_some_and(|active| active.address == address)
    }

    fn active_client_address(&self) -> Option<Address> {
        self.active_session.as_ref().map(|active| active.address)
    }

    fn enqueue_tx_chunks(&self, tx_chunks: Vec<Vec<u8>>) {
        let Some(active) = self.active_session.as_ref() else {
            warn!("no active TX subscriber; cannot send response");
            return;
        };

        let chunk_count = tx_chunks.len();
        let byte_count: usize = tx_chunks.iter().map(Vec::len).sum();
        if let Err(error) = active.tx_sender.send(tx_chunks) {
            warn!(
                %error,
                client_address = %active.address,
                "failed to queue TX chunks"
            );
            return;
        }

        info!(
            client_address = %active.address,
            chunk_count,
            bytes = byte_count,
            "queued TX chunks"
        );
    }

    fn record_tx_chunk_sent(&mut self, address: Address, session_token: u64) -> bool {
        let is_current = self.active_session.as_ref().is_some_and(|active| {
            active.address == address && active.session_token == session_token
        });
        if is_current {
            self.tx_sent_count += 1;
        }
        is_current
    }

    fn allocate_session_token(&mut self) -> u64 {
        let token = self.next_session_token;
        self.next_session_token = self.next_session_token.wrapping_add(1).max(1);
        token
    }
}

struct BleActiveSession {
    address: Address,
    adapter_name: String,
    mtu: usize,
    session_token: u64,
    tx_sender: mpsc::UnboundedSender<Vec<Vec<u8>>>,
    _gate_session: ActiveSession,
}

struct BleGattStatusSnapshot {
    busy: bool,
    active_client: Option<BleActiveClientSnapshot>,
    rx_count: u64,
    tx_sent_count: u64,
}

struct BleActiveClientSnapshot {
    address: Address,
    adapter_name: String,
    mtu: usize,
    session_token: u64,
}

#[derive(Debug, Clone)]
struct BleAdapterSnapshot {
    name: String,
    address: String,
}

impl From<&BleActiveSession> for BleActiveClientSnapshot {
    fn from(session: &BleActiveSession) -> Self {
        Self {
            address: session.address,
            adapter_name: session.adapter_name.clone(),
            mtu: session.mtu,
            session_token: session.session_token,
        }
    }
}

pub async fn run_ble_gatt<P>(
    router: RequestRouter<P>,
    options: BleGattOptions,
) -> anyhow::Result<()>
where
    P: PlayerBackend,
{
    info!("starting Matchbox BLE GATT transport");

    let state = Arc::new(RwLock::new(BleGattState::new(router)));
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

    if let Err(error) = adapter.set_alias(options.device_name.clone()).await {
        warn!(%error, "failed to set adapter alias; continuing");
    }

    let adapter_name = adapter.name().to_string();
    let address = adapter.address().await?;
    let adapter_snapshot = BleAdapterSnapshot {
        name: adapter_name.clone(),
        address: address.to_string(),
    };
    info!(adapter = %adapter_name, %address, "using Bluetooth adapter");

    let (app, tx_control) = build_gatt_application(Arc::clone(&state), options.clone());
    let _app_handle = adapter
        .serve_gatt_application(app)
        .await
        .context("failed to register GATT application")?;
    info!(service_uuid = %SERVICE_UUID, "registered Matchbox GATT service");

    tokio::spawn(handle_tx_subscriptions(Arc::clone(&state), tx_control));

    let advertisement = Advertisement {
        service_uuids: vec![SERVICE_UUID].into_iter().collect(),
        local_name: Some(options.device_name.clone()),
        discoverable: Some(true),
        ..Default::default()
    };
    let _adv_handle = adapter
        .advertise(advertisement)
        .await
        .context("failed to start BLE advertisement")?;
    info!(name = %options.device_name, "advertising started");

    let _control_socket = match options.control_socket.clone() {
        Some(path) => Some(start_ble_control_socket(
            path,
            Arc::clone(&state),
            options.clone(),
            adapter_snapshot,
        )?),
        None => None,
    };

    tokio::signal::ctrl_c()
        .await
        .context("failed waiting for Ctrl-C")?;

    info!("stopping Matchbox BLE GATT transport");
    Ok(())
}

fn start_ble_control_socket<P>(
    path: PathBuf,
    state: Arc<RwLock<BleGattState<P>>>,
    options: BleGattOptions,
    adapter: BleAdapterSnapshot,
) -> anyhow::Result<crate::ControlSocketGuard>
where
    P: PlayerBackend,
{
    let handler: ControlHandler = Arc::new(move |request| {
        let state = Arc::clone(&state);
        let options = options.clone();
        let adapter = adapter.clone();
        async move { handle_control_request(state, options, adapter, request).await }.boxed()
    });
    start_control_socket(path, handler)
}

async fn handle_control_request<P>(
    state: Arc<RwLock<BleGattState<P>>>,
    options: BleGattOptions,
    adapter: BleAdapterSnapshot,
    request: BtControlRequest,
) -> BtControlResponse
where
    P: PlayerBackend,
{
    match request.method.as_str() {
        BT_CONTROL_METHOD_STATUS => {
            let state = state.read().await;
            BtControlResponse::ok_status(bt_status(&options, &adapter, &state.status_snapshot()))
        }
        method => BtControlResponse::error(
            "unsupported_method",
            format!("unsupported bt control method: {method}"),
        ),
    }
}

fn bt_status(
    options: &BleGattOptions,
    adapter: &BleAdapterSnapshot,
    snapshot: &BleGattStatusSnapshot,
) -> BtStatus {
    BtStatus {
        service: SERVICE_NAME.to_string(),
        transport: "mba-bt-ble-local".to_string(),
        device_name: options.device_name.clone(),
        adapter: Some(BtAdapterStatus {
            name: adapter.name.clone(),
            address: adapter.address.clone(),
        }),
        advertising: true,
        service_uuid: SERVICE_UUID.to_string(),
        pairing_state: options.pairing_state.clone(),
        busy: snapshot.busy,
        active_client: snapshot
            .active_client
            .as_ref()
            .map(|client| BtActiveClientStatus {
                address: client.address.to_string(),
                adapter: client.adapter_name.clone(),
                mtu: client.mtu,
                session_token: client.session_token,
            }),
        rx_chunk_writes: snapshot.rx_count,
        tx_chunks_sent: snapshot.tx_sent_count,
    }
}

fn build_gatt_application<P>(
    state: Arc<RwLock<BleGattState<P>>>,
    options: BleGattOptions,
) -> (Application, CharacteristicControl)
where
    P: PlayerBackend,
{
    let status_options = options.clone();
    let status_state = Arc::clone(&state);
    let status_read = CharacteristicRead {
        read: true,
        fun: Box::new(move |req| {
            let status_options = status_options.clone();
            let status_state = Arc::clone(&status_state);
            Box::pin(async move {
                let state = status_state.read().await;
                let snapshot = state.status_snapshot();
                info!(
                    client_address = %req.device_address,
                    mtu = req.mtu,
                    busy = snapshot.busy,
                    "Status characteristic read"
                );
                Ok(status_payload(&status_options, &snapshot))
            })
        }),
        ..Default::default()
    };

    let rx_state = Arc::clone(&state);
    let rx_write = CharacteristicWrite {
        write: true,
        method: CharacteristicWriteMethod::Fun(Box::new(move |new_value, req| {
            let rx_state = Arc::clone(&rx_state);
            Box::pin(async move {
                handle_rx_chunk(rx_state, req.device_address, req.mtu, new_value).await
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

async fn handle_tx_subscriptions<P>(
    state: Arc<RwLock<BleGattState<P>>>,
    mut tx_control: CharacteristicControl,
) where
    P: PlayerBackend,
{
    while let Some(event) = tx_control.next().await {
        match event {
            CharacteristicControlEvent::Notify(writer) => {
                handle_tx_subscription(Arc::clone(&state), writer).await;
            }
            CharacteristicControlEvent::Write(_) => {
                debug!("ignoring write event on TX characteristic");
            }
        }
    }
}

async fn handle_tx_subscription<P>(
    state: Arc<RwLock<BleGattState<P>>>,
    writer: CharacteristicWriter,
) where
    P: PlayerBackend,
{
    let adapter_name = writer.adapter_name().to_string();
    let address = writer.device_address();
    let mtu = writer.mtu();

    let session = {
        let mut state = state.write().await;
        state.start_tx_session(adapter_name.clone(), address, mtu)
    };

    match session {
        Ok((session_token, tx_receiver)) => {
            info!(
                client_address = %address,
                adapter = %adapter_name,
                mtu,
                session_token,
                "client subscribed to TX notifications"
            );
            tokio::spawn(run_tx_session(
                Arc::clone(&state),
                writer,
                address,
                session_token,
                tx_receiver,
            ));
        }
        Err(error) => {
            warn!(
                client_address = %address,
                adapter = %adapter_name,
                mtu,
                ?error,
                "rejected TX notification subscription"
            );
            tokio::spawn(send_busy_response(writer, error));
        }
    }
}

async fn run_tx_session<P>(
    state: Arc<RwLock<BleGattState<P>>>,
    writer: CharacteristicWriter,
    address: Address,
    session_token: u64,
    mut tx_receiver: mpsc::UnboundedReceiver<Vec<Vec<u8>>>,
) where
    P: PlayerBackend,
{
    loop {
        tokio::select! {
            closed = writer.closed() => {
                match closed {
                    Ok(()) => info!(
                        client_address = %address,
                        session_token,
                        "TX notification session closed"
                    ),
                    Err(error) => warn!(
                        client_address = %address,
                        session_token,
                        %error,
                        "TX notification session close wait failed"
                    ),
                }
                break;
            }
            maybe_chunks = tx_receiver.recv() => {
                let Some(chunks) = maybe_chunks else {
                    debug!(
                        client_address = %address,
                        session_token,
                        "TX queue closed"
                    );
                    break;
                };

                for chunk in chunks {
                    if let Err(error) = writer.send(&chunk).await {
                        warn!(
                            client_address = %address,
                            session_token,
                            %error,
                            "failed to send TX chunk"
                        );
                        cleanup_tx_session(Arc::clone(&state), address, session_token).await;
                        return;
                    }
                    record_tx_chunk_sent(Arc::clone(&state), address, session_token).await;
                    info!(
                        client_address = %address,
                        session_token,
                        bytes = chunk.len(),
                        "sent TX chunk"
                    );
                }
            }
        }
    }

    cleanup_tx_session(state, address, session_token).await;
}

async fn record_tx_chunk_sent<P>(
    state: Arc<RwLock<BleGattState<P>>>,
    address: Address,
    session_token: u64,
) where
    P: PlayerBackend,
{
    let mut state = state.write().await;
    if !state.record_tx_chunk_sent(address, session_token) {
        debug!(
            client_address = %address,
            session_token,
            "ignoring stale TX sent count"
        );
    }
}

async fn cleanup_tx_session<P>(
    state: Arc<RwLock<BleGattState<P>>>,
    address: Address,
    session_token: u64,
) where
    P: PlayerBackend,
{
    let mut state = state.write().await;
    if state.finish_tx_session(address, session_token) {
        info!(
            client_address = %address,
            session_token,
            "cleared active BLE session"
        );
    } else {
        debug!(
            client_address = %address,
            session_token,
            "ignoring stale BLE session cleanup"
        );
    }
}

async fn send_busy_response(writer: CharacteristicWriter, error: ProtocolError) {
    let address = writer.device_address();
    let message = ProtocolMessage::error_response(0, error);
    let bytes = match message.to_json_bytes() {
        Ok(bytes) => bytes,
        Err(error) => {
            warn!(
                client_address = %address,
                %error,
                "failed to encode busy response"
            );
            return;
        }
    };
    let chunks = match ble::encode_chunks(1, &bytes) {
        Ok(chunks) => chunks,
        Err(error) => {
            warn!(
                client_address = %address,
                %error,
                "failed to chunk busy response"
            );
            return;
        }
    };

    for chunk in chunks {
        if let Err(error) = writer.send(&chunk).await {
            warn!(
                client_address = %address,
                %error,
                "failed to send busy response"
            );
            return;
        }
        info!(
            client_address = %address,
            bytes = chunk.len(),
            "sent busy response chunk"
        );
    }
}

async fn handle_rx_chunk<P>(
    state: Arc<RwLock<BleGattState<P>>>,
    address: Address,
    mtu: u16,
    chunk: Vec<u8>,
) -> Result<(), ReqError>
where
    P: PlayerBackend,
{
    let mut state = state.write().await;
    if !state.active_client_allows(address) {
        let active_address = state.active_client_address();
        warn!(
            client_address = %address,
            active_client_address = active_address.map(|address| address.to_string()),
            mtu,
            bytes = chunk.len(),
            "rejected RX write from inactive client"
        );
        return Err(ReqError::NotAuthorized);
    }

    state.rx_count += 1;
    let rx_write_count = state.rx_count;
    info!(
        client_address = %address,
        rx_write_count,
        mtu,
        bytes = chunk.len(),
        "received RX chunk"
    );
    debug!(
        client_address = %address,
        rx_write_count,
        chunk = ?chunk,
        "RX chunk bytes"
    );

    match receive_chunk_locked(&mut state, &chunk).await {
        Ok(Some(tx_chunks)) => state.enqueue_tx_chunks(tx_chunks),
        Ok(None) => {}
        Err(error) => {
            warn!(%error, "rejected RX chunk");
            let message = bad_request_for_transport_error(error);
            match state.transport.encode_outbound_message(&message) {
                Ok(tx_chunks) => state.enqueue_tx_chunks(tx_chunks),
                Err(error) => warn!(%error, "failed to encode transport error response"),
            }
        }
    }

    Ok(())
}

async fn receive_chunk_locked<P>(
    state: &mut BleGattState<P>,
    chunk: &[u8],
) -> Result<Option<Vec<Vec<u8>>>, BleTransportError>
where
    P: PlayerBackend,
{
    let router = state.router.clone();
    state.transport.receive_chunk(&router, chunk).await
}

fn status_payload(options: &BleGattOptions, snapshot: &BleGattStatusSnapshot) -> Vec<u8> {
    let active_client = snapshot.active_client.as_ref().map(|client| {
        json!({
            "address": client.address.to_string(),
            "adapter": client.adapter_name,
            "mtu": client.mtu,
            "session_token": client.session_token,
        })
    });

    json!({
        "service": mba_protocol::SERVICE_NAME,
        "transport": "mba-bt-ble-local",
        "device_name": options.device_name,
        "protocol_versions": [APP_PROTOCOL_VERSION],
        "chunk_protocol_version": ble::CHUNK_VERSION,
        "max_message_bytes": MAX_MESSAGE_BYTES,
        "target_response_bytes": TARGET_RESPONSE_BYTES,
        "target_gatt_value_bytes": ble::TARGET_GATT_VALUE_BYTES,
        "chunk_header_bytes": ble::CHUNK_HEADER_BYTES,
        "target_chunk_payload_bytes": ble::TARGET_CHUNK_PAYLOAD_BYTES,
        "default_page_limit": DEFAULT_PAGE_LIMIT,
        "max_page_limit": MAX_PAGE_LIMIT,
        "one_in_flight_per_direction": true,
        "pairing_state": options.pairing_state,
        "busy": snapshot.busy,
        "active_client": active_client,
        "rx_chunk_writes": snapshot.rx_count,
        "tx_chunks_sent": snapshot.tx_sent_count,
    })
    .to_string()
    .into_bytes()
}

fn client_id_for_address(address: Address) -> u64 {
    let mut bytes = [0u8; 8];
    bytes[2..].copy_from_slice(&address.0);
    u64::from_be_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use mba_protocol::ErrorCode;
    use serde_json::Value;

    use super::*;
    use crate::FakePlayerBackend;

    fn test_state() -> BleGattState<FakePlayerBackend> {
        BleGattState::new(RequestRouter::new(FakePlayerBackend::ready()))
    }

    #[test]
    fn client_id_is_derived_from_bluetooth_address_bytes() {
        let address = Address::new([0x01, 0x23, 0x45, 0x67, 0x89, 0xab]);

        assert_eq!(client_id_for_address(address), 0x0000_0123_4567_89ab);
    }

    #[tokio::test]
    async fn session_state_rejects_second_client_and_cleans_up_current_client() {
        let mut state = test_state();
        let first = Address::new([1, 2, 3, 4, 5, 6]);
        let second = Address::new([6, 5, 4, 3, 2, 1]);

        let (first_token, mut first_rx) = state
            .start_tx_session("hci0".to_string(), first, 517)
            .expect("first client starts a session");
        assert_eq!(first_token, 1);
        assert!(state.active_client_allows(first));
        assert!(!state.active_client_allows(second));

        let busy = state
            .start_tx_session("hci0".to_string(), second, 517)
            .expect_err("second client is busy");
        assert_eq!(busy.code, ErrorCode::Busy);
        assert_eq!(state.next_session_token, 2);
        assert!(state.active_client_allows(first));

        let (refreshed_token, _refreshed_rx) = state
            .start_tx_session("hci0".to_string(), first, 517)
            .expect("same client can refresh its notification session");
        assert_eq!(refreshed_token, 2);
        assert_eq!(first_rx.recv().await, None);
        assert!(!state.finish_tx_session(first, first_token));
        assert!(state.active_client_allows(first));

        assert!(state.finish_tx_session(first, refreshed_token));
        assert!(!state.active_client_allows(first));
        assert!(state
            .start_tx_session("hci0".to_string(), second, 247)
            .is_ok());
    }

    #[test]
    fn status_payload_reports_active_client() {
        let mut state = test_state();
        let address = Address::new([1, 2, 3, 4, 5, 6]);
        state
            .start_tx_session("hci0".to_string(), address, 517)
            .expect("client starts a session");

        let payload = status_payload(&BleGattOptions::default(), &state.status_snapshot());
        let status: Value = serde_json::from_slice(&payload).expect("status JSON parses");

        assert_eq!(status["busy"], true);
        assert_eq!(status["active_client"]["address"], "01:02:03:04:05:06");
        assert_eq!(status["active_client"]["adapter"], "hci0");
        assert_eq!(status["active_client"]["mtu"], 517);
    }
}
