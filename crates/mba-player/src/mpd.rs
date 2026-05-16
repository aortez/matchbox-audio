use std::{net::SocketAddr, time::Duration};

use mba_protocol::{basename, PlaybackInfo, PlaybackState, QueueItem, QueueListing, TrackInfo};
use mpd_client::{
    client::{ConnectionEvent, ConnectionEvents, Subsystem},
    commands,
    protocol::command::Command as RawCommand,
    responses::{PlayState, SongInQueue, Status},
    Client,
};
use tokio::{
    net::TcpStream,
    select,
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
    time::sleep,
};
use tracing::{debug, info, warn};

const COMMAND_QUEUE: usize = 32;
const RECONNECT_INITIAL: Duration = Duration::from_millis(500);
const RECONNECT_MAX: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub enum MpdError {
    Unavailable,
    Command(String),
    ChannelClosed,
}

impl std::fmt::Display for MpdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => f.write_str("MPD is not currently reachable"),
            Self::Command(message) => write!(f, "MPD rejected the command: {message}"),
            Self::ChannelClosed => f.write_str("MPD client task is gone"),
        }
    }
}

impl std::error::Error for MpdError {}

#[derive(Debug)]
enum Action {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
    Seek {
        seconds: f64,
    },
    SetVolume {
        level: u8,
    },
    PlayQueue(QueueItemTarget),
    RemoveQueue(QueueItemTarget),
    MoveQueue {
        target: QueueItemTarget,
        to_position: u32,
    },
    MoveQueueAfterCurrent(QueueItemTarget),
}

#[derive(Debug)]
enum Request {
    Action {
        action: Action,
        reply: oneshot::Sender<Result<(), MpdError>>,
    },
    Rescan {
        reply: oneshot::Sender<Result<u64, MpdError>>,
    },
    Enqueue {
        paths: Vec<String>,
        reply: oneshot::Sender<Result<(), MpdError>>,
    },
    ClearQueue {
        reply: oneshot::Sender<Result<(), MpdError>>,
    },
    ListQueue {
        reply: oneshot::Sender<Result<QueueListing, MpdError>>,
    },
}

impl Request {
    fn reject(self, error: MpdError) {
        match self {
            Request::Action { reply, .. } => {
                let _ = reply.send(Err(error));
            }
            Request::Rescan { reply } => {
                let _ = reply.send(Err(error));
            }
            Request::Enqueue { reply, .. } | Request::ClearQueue { reply } => {
                let _ = reply.send(Err(error));
            }
            Request::ListQueue { reply } => {
                let _ = reply.send(Err(error));
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueItemTarget {
    Id(u64),
    Position(u32),
}

#[derive(Clone)]
pub struct MpdHandle {
    state: watch::Receiver<Option<PlaybackInfo>>,
    commands: mpsc::Sender<Request>,
}

impl MpdHandle {
    pub fn snapshot(&self) -> Option<PlaybackInfo> {
        self.state.borrow().clone()
    }

    pub async fn play(&self) -> Result<(), MpdError> {
        self.send_action(Action::Play).await
    }

    pub async fn pause(&self) -> Result<(), MpdError> {
        self.send_action(Action::Pause).await
    }

    pub async fn toggle(&self) -> Result<(), MpdError> {
        self.send_action(Action::Toggle).await
    }

    pub async fn stop(&self) -> Result<(), MpdError> {
        self.send_action(Action::Stop).await
    }

    pub async fn next(&self) -> Result<(), MpdError> {
        self.send_action(Action::Next).await
    }

    pub async fn previous(&self) -> Result<(), MpdError> {
        self.send_action(Action::Previous).await
    }

    pub async fn seek(&self, seconds: f64) -> Result<(), MpdError> {
        self.send_action(Action::Seek { seconds }).await
    }

    pub async fn set_volume(&self, level: u8) -> Result<(), MpdError> {
        self.send_action(Action::SetVolume { level }).await
    }

    pub async fn play_queue_item(&self, target: QueueItemTarget) -> Result<(), MpdError> {
        self.send_action(Action::PlayQueue(target)).await
    }

    pub async fn remove_queue_item(&self, target: QueueItemTarget) -> Result<(), MpdError> {
        self.send_action(Action::RemoveQueue(target)).await
    }

    pub async fn move_queue_item(
        &self,
        target: QueueItemTarget,
        to_position: u32,
    ) -> Result<(), MpdError> {
        self.send_action(Action::MoveQueue {
            target,
            to_position,
        })
        .await
    }

    pub async fn move_queue_item_after_current(
        &self,
        target: QueueItemTarget,
    ) -> Result<(), MpdError> {
        self.send_action(Action::MoveQueueAfterCurrent(target))
            .await
    }

    pub async fn rescan(&self) -> Result<u64, MpdError> {
        let (tx, rx) = oneshot::channel();
        self.commands
            .send(Request::Rescan { reply: tx })
            .await
            .map_err(|_| MpdError::ChannelClosed)?;
        rx.await.map_err(|_| MpdError::ChannelClosed)?
    }

    pub async fn enqueue(&self, path: String) -> Result<(), MpdError> {
        self.enqueue_paths(vec![path]).await
    }

    pub async fn enqueue_paths(&self, paths: Vec<String>) -> Result<(), MpdError> {
        let (tx, rx) = oneshot::channel();
        self.commands
            .send(Request::Enqueue { paths, reply: tx })
            .await
            .map_err(|_| MpdError::ChannelClosed)?;
        rx.await.map_err(|_| MpdError::ChannelClosed)?
    }

    pub async fn clear_queue(&self) -> Result<(), MpdError> {
        let (tx, rx) = oneshot::channel();
        self.commands
            .send(Request::ClearQueue { reply: tx })
            .await
            .map_err(|_| MpdError::ChannelClosed)?;
        rx.await.map_err(|_| MpdError::ChannelClosed)?
    }

    pub async fn list_queue(&self) -> Result<QueueListing, MpdError> {
        let (tx, rx) = oneshot::channel();
        self.commands
            .send(Request::ListQueue { reply: tx })
            .await
            .map_err(|_| MpdError::ChannelClosed)?;
        rx.await.map_err(|_| MpdError::ChannelClosed)?
    }

    async fn send_action(&self, action: Action) -> Result<(), MpdError> {
        let (tx, rx) = oneshot::channel();
        self.commands
            .send(Request::Action { action, reply: tx })
            .await
            .map_err(|_| MpdError::ChannelClosed)?;
        rx.await.map_err(|_| MpdError::ChannelClosed)?
    }
}

pub fn start(addr: SocketAddr) -> (MpdHandle, JoinHandle<()>) {
    let (state_tx, state_rx) = watch::channel(None);
    let (cmd_tx, cmd_rx) = mpsc::channel(COMMAND_QUEUE);
    let join = tokio::spawn(actor_loop(addr, cmd_rx, state_tx));
    (
        MpdHandle {
            state: state_rx,
            commands: cmd_tx,
        },
        join,
    )
}

async fn actor_loop(
    addr: SocketAddr,
    mut commands: mpsc::Receiver<Request>,
    state: watch::Sender<Option<PlaybackInfo>>,
) {
    let mut backoff = RECONNECT_INITIAL;
    loop {
        let (client, mut events) = match connect(addr).await {
            Ok(pair) => pair,
            Err(error) => {
                warn!(%error, %addr, "MPD connect failed; will retry");
                let _ = state.send(None);
                if !drain_during_backoff(&mut commands, backoff).await {
                    info!("MPD client task shutting down during reconnect");
                    return;
                }
                backoff = (backoff * 2).min(RECONNECT_MAX);
                continue;
            }
        };
        backoff = RECONNECT_INITIAL;
        info!(%addr, "MPD client connected");
        publish_state(&client, &state).await;

        let disconnected = session_loop(&client, &mut events, &mut commands, &state).await;
        let _ = state.send(None);
        match disconnected {
            SessionEnd::Disconnected => {
                warn!("MPD connection closed; reconnecting");
                continue;
            }
            SessionEnd::Shutdown => {
                info!("MPD client task shutting down");
                return;
            }
        }
    }
}

// Returns false if the command channel closed (shutdown), true if the backoff elapsed.
async fn drain_during_backoff(commands: &mut mpsc::Receiver<Request>, backoff: Duration) -> bool {
    let timer = sleep(backoff);
    tokio::pin!(timer);
    loop {
        select! {
            _ = &mut timer => return true,
            req = commands.recv() => {
                match req {
                    Some(req) => req.reject(MpdError::Unavailable),
                    None => return false,
                }
            }
        }
    }
}

enum SessionEnd {
    Disconnected,
    Shutdown,
}

async fn session_loop(
    client: &Client,
    events: &mut ConnectionEvents,
    commands: &mut mpsc::Receiver<Request>,
    state: &watch::Sender<Option<PlaybackInfo>>,
) -> SessionEnd {
    loop {
        select! {
            request = commands.recv() => {
                match request {
                    Some(request) => {
                        if !handle_request(client, request).await {
                            return SessionEnd::Disconnected;
                        }
                    }
                    None => return SessionEnd::Shutdown,
                }
            }
            event = events.next() => {
                match event {
                    Some(ConnectionEvent::SubsystemChange(subsystem)) => {
                        if subsystem_affects_playback(&subsystem) {
                            publish_state(client, state).await;
                        } else {
                            debug!(?subsystem, "ignoring subsystem change");
                        }
                    }
                    Some(ConnectionEvent::ConnectionClosed(error)) => {
                        warn!(%error, "MPD reported connection closed");
                        return SessionEnd::Disconnected;
                    }
                    None => return SessionEnd::Disconnected,
                }
            }
        }
    }
}

async fn connect(addr: SocketAddr) -> Result<(Client, ConnectionEvents), String> {
    let stream = TcpStream::connect(addr)
        .await
        .map_err(|e| format!("tcp connect: {e}"))?;
    Client::connect(stream)
        .await
        .map_err(|e| format!("mpd handshake: {e}"))
}

async fn handle_request(client: &Client, request: Request) -> bool {
    match request {
        Request::Action { action, reply } => handle_action(client, action, reply).await,
        Request::Rescan { reply } => handle_rescan(client, reply).await,
        Request::Enqueue { paths, reply } => handle_enqueue(client, paths, reply).await,
        Request::ClearQueue { reply } => handle_clear_queue(client, reply).await,
        Request::ListQueue { reply } => handle_list_queue(client, reply).await,
    }
}

async fn handle_action(
    client: &Client,
    action: Action,
    reply: oneshot::Sender<Result<(), MpdError>>,
) -> bool {
    let outcome = match action {
        Action::Play => client
            .command(commands::Play::current())
            .await
            .map_err(|e| e.to_string()),
        Action::Pause => client
            .command(commands::SetPause(true))
            .await
            .map_err(|e| e.to_string()),
        Action::Toggle => match client.command(commands::Status).await {
            Ok(status) => match status.state {
                PlayState::Playing => client
                    .command(commands::SetPause(true))
                    .await
                    .map_err(|e| e.to_string()),
                PlayState::Paused | PlayState::Stopped => client
                    .command(commands::Play::current())
                    .await
                    .map_err(|e| e.to_string()),
            },
            Err(e) => Err(e.to_string()),
        },
        Action::Stop => client
            .command(commands::Stop)
            .await
            .map_err(|e| e.to_string()),
        Action::Next => client
            .command(commands::Next)
            .await
            .map_err(|e| e.to_string()),
        Action::Previous => client
            .command(commands::Previous)
            .await
            .map_err(|e| e.to_string()),
        Action::Seek { seconds } => {
            let mode = commands::SeekMode::Absolute(Duration::from_secs_f64(seconds.max(0.0)));
            client
                .command(commands::Seek(mode))
                .await
                .map_err(|e| e.to_string())
        }
        Action::SetVolume { level } => client
            .command(commands::SetVolume(level.min(100)))
            .await
            .map_err(|e| e.to_string()),
        Action::PlayQueue(target) => match target {
            QueueItemTarget::Id(id) => client
                .command(commands::Play::song(commands::SongId(id)))
                .await
                .map_err(|e| e.to_string()),
            QueueItemTarget::Position(position) => client
                .command(commands::Play::song(commands::SongPosition(
                    position as usize,
                )))
                .await
                .map_err(|e| e.to_string()),
        },
        Action::RemoveQueue(target) => remove_queue_item(client, target).await,
        Action::MoveQueue {
            target,
            to_position,
        } => move_queue_item(client, target, to_position).await,
        Action::MoveQueueAfterCurrent(target) => {
            move_queue_item_after_current(client, target).await
        }
    };

    match outcome {
        Ok(()) => {
            let _ = reply.send(Ok(()));
        }
        Err(message) => {
            warn!(%message, "MPD command failed");
            let _ = reply.send(Err(MpdError::Command(message)));
        }
    }
    true
}

async fn remove_queue_item(client: &Client, target: QueueItemTarget) -> Result<(), String> {
    match target {
        QueueItemTarget::Id(id) => client
            .command(commands::Delete::id(commands::SongId(id)))
            .await
            .map_err(|e| e.to_string()),
        QueueItemTarget::Position(position) => client
            .command(commands::Delete::position(commands::SongPosition(
                position as usize,
            )))
            .await
            .map_err(|e| e.to_string()),
    }
}

async fn move_queue_item(
    client: &Client,
    target: QueueItemTarget,
    to_position: u32,
) -> Result<(), String> {
    client
        .command(move_queue_command(target, to_position))
        .await
        .map_err(|e| e.to_string())
}

async fn move_queue_item_after_current(
    client: &Client,
    target: QueueItemTarget,
) -> Result<(), String> {
    let status = client
        .command(commands::Status)
        .await
        .map_err(|e| e.to_string())?;

    if queue_target_matches_current(target, status.current_song) {
        return Ok(());
    }

    let command = if status.current_song.is_some() {
        move_queue_command_after_current(target)
    } else {
        move_queue_command(target, 0)
    };

    client.command(command).await.map_err(|e| e.to_string())
}

fn move_queue_command(target: QueueItemTarget, to_position: u32) -> commands::Move {
    let to_position = commands::SongPosition(to_position as usize);
    match target {
        QueueItemTarget::Id(id) => {
            commands::Move::id(commands::SongId(id)).to_position(to_position)
        }
        QueueItemTarget::Position(position) => {
            commands::Move::position(commands::SongPosition(position as usize))
                .to_position(to_position)
        }
    }
}

fn move_queue_command_after_current(target: QueueItemTarget) -> commands::Move {
    match target {
        QueueItemTarget::Id(id) => commands::Move::id(commands::SongId(id)).after_current(0),
        QueueItemTarget::Position(position) => {
            commands::Move::position(commands::SongPosition(position as usize)).after_current(0)
        }
    }
}

fn queue_target_matches_current(
    target: QueueItemTarget,
    current: Option<(commands::SongPosition, commands::SongId)>,
) -> bool {
    match (target, current) {
        (QueueItemTarget::Id(id), Some((_, current_id))) => id == current_id.0,
        (QueueItemTarget::Position(position), Some((current_position, _))) => {
            position as usize == current_position.0
        }
        _ => false,
    }
}

async fn handle_rescan(client: &Client, reply: oneshot::Sender<Result<u64, MpdError>>) -> bool {
    match client.command(commands::Update::new()).await {
        Ok(job_id) => {
            let _ = reply.send(Ok(job_id));
        }
        Err(error) => {
            let message = error.to_string();
            warn!(%message, "MPD update failed");
            let _ = reply.send(Err(MpdError::Command(message)));
        }
    }
    true
}

async fn handle_enqueue(
    client: &Client,
    paths: Vec<String>,
    reply: oneshot::Sender<Result<(), MpdError>>,
) -> bool {
    match enqueue_and_autoplay_if_empty(client, paths).await {
        Ok(()) => {
            let _ = reply.send(Ok(()));
        }
        Err(message) => {
            warn!(%message, "MPD add failed");
            let _ = reply.send(Err(MpdError::Command(message)));
        }
    }
    true
}

async fn enqueue_and_autoplay_if_empty(client: &Client, paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let total_paths = paths.len();
    let before = client
        .command(commands::Status)
        .await
        .map_err(|e| e.to_string())?;

    let mut added_count = 0;
    for path in paths {
        let command = match command_with_optional_arg("add", &path) {
            Ok(command) => command,
            Err(error) => {
                return finish_enqueue_failure(
                    client,
                    before.playlist_length,
                    total_paths,
                    added_count,
                    error,
                )
                .await;
            }
        };
        if let Err(error) = client.raw_command(command).await {
            return finish_enqueue_failure(
                client,
                before.playlist_length,
                total_paths,
                added_count,
                error.to_string(),
            )
            .await;
        }
        added_count += 1;
    }

    if let Err(error) =
        autoplay_after_enqueue_if_needed(before.playlist_length, added_count, client).await
    {
        return Err(post_enqueue_reconciliation_failure(
            total_paths,
            added_count,
            &error,
        ));
    }

    Ok(())
}

async fn finish_enqueue_failure(
    client: &Client,
    previous_queue_length: usize,
    total_paths: usize,
    added_count: usize,
    error: String,
) -> Result<(), String> {
    if added_count == 0 {
        return Err(error);
    }

    let autoplay_error =
        autoplay_after_enqueue_if_needed(previous_queue_length, added_count, client)
            .await
            .err();
    Err(partial_enqueue_failure(
        total_paths,
        added_count,
        &error,
        autoplay_error.as_deref(),
    ))
}

async fn autoplay_after_enqueue_if_needed(
    previous_queue_length: usize,
    added_count: usize,
    client: &Client,
) -> Result<(), String> {
    if added_count == 0 {
        return Ok(());
    }

    if should_autoplay_after_enqueue(previous_queue_length, client).await? {
        client
            .command(commands::Play::current())
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn partial_enqueue_failure(
    total_paths: usize,
    added_count: usize,
    error: &str,
    autoplay_error: Option<&str>,
) -> String {
    let mut message = format!(
        "MPD rejected directory enqueue after adding {added_count} of {total_paths} tracks; queue may be partially updated: {error}"
    );
    if let Some(autoplay_error) = autoplay_error {
        message.push_str("; playback reconciliation also failed: ");
        message.push_str(autoplay_error);
    }
    message
}

fn post_enqueue_reconciliation_failure(
    total_paths: usize,
    added_count: usize,
    error: &str,
) -> String {
    format!(
        "MPD accepted {added_count} of {total_paths} queued tracks, but playback reconciliation failed: {error}"
    )
}

async fn should_autoplay_after_enqueue(
    previous_queue_length: usize,
    client: &Client,
) -> Result<bool, String> {
    if previous_queue_length != 0 {
        return Ok(false);
    }
    let after = client
        .command(commands::Status)
        .await
        .map_err(|e| e.to_string())?;
    Ok(queue_transition_should_autoplay(
        previous_queue_length,
        after.playlist_length,
    ))
}

fn queue_transition_should_autoplay(
    previous_queue_length: usize,
    current_queue_length: usize,
) -> bool {
    previous_queue_length == 0 && current_queue_length > 0
}

async fn handle_clear_queue(client: &Client, reply: oneshot::Sender<Result<(), MpdError>>) -> bool {
    send_raw_unit_command(client, RawCommand::new("clear"), reply, "MPD clear failed").await;
    true
}

async fn handle_list_queue(
    client: &Client,
    reply: oneshot::Sender<Result<QueueListing, MpdError>>,
) -> bool {
    match client.raw_command(RawCommand::new("playlistinfo")).await {
        Ok(frame) => {
            let queue = parse_queue(frame.fields());
            let _ = reply.send(Ok(queue));
        }
        Err(error) => {
            let message = error.to_string();
            warn!(%message, "MPD playlistinfo failed");
            let _ = reply.send(Err(MpdError::Command(message)));
        }
    }
    true
}

async fn send_raw_unit_command(
    client: &Client,
    command: RawCommand,
    reply: oneshot::Sender<Result<(), MpdError>>,
    log_message: &str,
) {
    match client.raw_command(command).await {
        Ok(_) => {
            let _ = reply.send(Ok(()));
        }
        Err(error) => {
            let message = error.to_string();
            warn!(%message, "{log_message}");
            let _ = reply.send(Err(MpdError::Command(message)));
        }
    }
}

fn command_with_optional_arg(command_name: &'static str, arg: &str) -> Result<RawCommand, String> {
    let mut command = RawCommand::new(command_name);
    if !arg.is_empty() {
        command
            .add_argument::<&str>(arg)
            .map_err(|error| error.to_string())?;
    }
    Ok(command)
}

fn subsystem_affects_playback(subsystem: &Subsystem) -> bool {
    matches!(
        subsystem,
        Subsystem::Player | Subsystem::Mixer | Subsystem::Queue
    )
}

async fn publish_state(client: &Client, state: &watch::Sender<Option<PlaybackInfo>>) {
    match read_playback(client).await {
        Ok(playback) => {
            let _ = state.send(Some(playback));
        }
        Err(error) => {
            warn!(%error, "MPD state refresh failed");
        }
    }
}

async fn read_playback(client: &Client) -> Result<PlaybackInfo, String> {
    let status = client
        .command(commands::Status)
        .await
        .map_err(|e| e.to_string())?;
    let current = client
        .command(commands::CurrentSong)
        .await
        .map_err(|e| e.to_string())?;
    Ok(playback_from(status, current))
}

fn playback_from(status: Status, current: Option<SongInQueue>) -> PlaybackInfo {
    let state = match status.state {
        PlayState::Playing => PlaybackState::Play,
        PlayState::Paused => PlaybackState::Pause,
        PlayState::Stopped => PlaybackState::Stop,
    };
    let (queue_position, queue_id, track) = match current {
        Some(entry) => {
            let queue_position = u32::try_from(entry.position.0).ok();
            let queue_id = Some(entry.id.0);
            let song = entry.song;
            let track = TrackInfo {
                uri: song.url.clone(),
                title: song.title().map(str::to_string),
                artist: song.artists().first().cloned(),
                album: song.album().map(str::to_string),
                duration_s: song.duration.map(|d| d.as_secs() as u32),
                elapsed_s: status.elapsed.map(|d| d.as_secs() as u32),
            };
            (queue_position, queue_id, Some(track))
        }
        None => (None, None, None),
    };
    PlaybackInfo {
        state,
        volume: status.volume,
        queue_position,
        queue_id,
        queue_length: status.playlist_length as u32,
        track,
    }
}

struct PendingQueueItem {
    uri: String,
    position: Option<u32>,
    id: Option<u64>,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration_s: Option<u32>,
}

impl PendingQueueItem {
    fn new(uri: String) -> Self {
        Self {
            uri,
            position: None,
            id: None,
            title: None,
            artist: None,
            album: None,
            duration_s: None,
        }
    }

    fn into_item(self, fallback_position: u32) -> QueueItem {
        QueueItem {
            position: self.position.unwrap_or(fallback_position),
            id: self.id,
            name: basename(&self.uri).to_string(),
            uri: self.uri,
            title: self.title,
            artist: self.artist,
            album: self.album,
            duration_s: self.duration_s,
        }
    }
}

fn parse_queue<'a, I>(fields: I) -> QueueListing
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut items = Vec::new();
    let mut pending: Option<PendingQueueItem> = None;

    for (key, value) in fields {
        match key {
            "file" => {
                if let Some(item) = pending.take() {
                    items.push(item.into_item(items.len() as u32));
                }
                pending = Some(PendingQueueItem::new(value.to_string()));
            }
            "Pos" => {
                if let Some(item) = pending.as_mut() {
                    item.position = value.parse::<u32>().ok();
                }
            }
            "Id" => {
                if let Some(item) = pending.as_mut() {
                    item.id = value.parse::<u64>().ok();
                }
            }
            "Title" => {
                if let Some(item) = pending.as_mut() {
                    item.title = Some(value.to_string());
                }
            }
            "Artist" => {
                if let Some(item) = pending.as_mut() {
                    item.artist = Some(value.to_string());
                }
            }
            "Album" => {
                if let Some(item) = pending.as_mut() {
                    item.album = Some(value.to_string());
                }
            }
            "Time" | "duration" => {
                if let Some(item) = pending.as_mut() {
                    if let Ok(secs) = value.parse::<f64>() {
                        item.duration_s = Some(secs.round().max(0.0) as u32);
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(item) = pending.take() {
        items.push(item.into_item(items.len() as u32));
    }

    QueueListing { items }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_queue_groups_playlistinfo_items() {
        let fields: &[(&str, &str)] = &[
            ("file", "Pink Floyd/Dark Side/01 Speak to Me.flac"),
            ("Title", "Speak to Me"),
            ("Artist", "Pink Floyd"),
            ("Album", "Dark Side"),
            ("Time", "91"),
            ("duration", "90.622"),
            ("Pos", "0"),
            ("Id", "10"),
            ("file", "single.flac"),
            ("Time", "214"),
            ("Pos", "1"),
            ("Id", "11"),
        ];
        let queue = parse_queue(fields.iter().copied());

        assert_eq!(queue.items.len(), 2);
        assert_eq!(queue.items[0].position, 0);
        assert_eq!(queue.items[0].id, Some(10));
        assert_eq!(
            queue.items[0].uri,
            "Pink Floyd/Dark Side/01 Speak to Me.flac"
        );
        assert_eq!(queue.items[0].name, "01 Speak to Me.flac");
        assert_eq!(queue.items[0].title.as_deref(), Some("Speak to Me"));
        assert_eq!(queue.items[0].artist.as_deref(), Some("Pink Floyd"));
        assert_eq!(queue.items[0].album.as_deref(), Some("Dark Side"));
        assert_eq!(queue.items[0].duration_s, Some(91));
        assert_eq!(queue.items[1].position, 1);
        assert_eq!(queue.items[1].name, "single.flac");
    }

    #[test]
    fn parse_queue_uses_fallback_position_when_missing() {
        let fields: &[(&str, &str)] = &[("file", "one.flac"), ("file", "two.flac")];
        let queue = parse_queue(fields.iter().copied());

        assert_eq!(queue.items[0].position, 0);
        assert_eq!(queue.items[1].position, 1);
    }

    #[test]
    fn autoplay_after_enqueue_only_when_queue_was_empty_and_gained_items() {
        assert!(queue_transition_should_autoplay(0, 1));
        assert!(queue_transition_should_autoplay(0, 12));
        assert!(!queue_transition_should_autoplay(0, 0));
        assert!(!queue_transition_should_autoplay(1, 2));
        assert!(!queue_transition_should_autoplay(3, 3));
    }

    #[test]
    fn partial_enqueue_failure_reports_partial_queue_state() {
        let message = partial_enqueue_failure(4, 2, "bad file", None);

        assert_eq!(
            message,
            "MPD rejected directory enqueue after adding 2 of 4 tracks; queue may be partially updated: bad file"
        );
    }

    #[test]
    fn partial_enqueue_failure_includes_reconciliation_failure() {
        let message = partial_enqueue_failure(3, 1, "bad file", Some("status failed"));

        assert_eq!(
            message,
            "MPD rejected directory enqueue after adding 1 of 3 tracks; queue may be partially updated: bad file; playback reconciliation also failed: status failed"
        );
    }

    #[test]
    fn post_enqueue_reconciliation_failure_reports_accepted_tracks() {
        let message = post_enqueue_reconciliation_failure(3, 3, "play failed");

        assert_eq!(
            message,
            "MPD accepted 3 of 3 queued tracks, but playback reconciliation failed: play failed"
        );
    }

    #[test]
    fn queue_target_matches_current_by_id_or_position() {
        let current = Some((commands::SongPosition(4), commands::SongId(99)));

        assert!(queue_target_matches_current(
            QueueItemTarget::Id(99),
            current
        ));
        assert!(queue_target_matches_current(
            QueueItemTarget::Position(4),
            current
        ));
        assert!(!queue_target_matches_current(
            QueueItemTarget::Id(100),
            current
        ));
        assert!(!queue_target_matches_current(
            QueueItemTarget::Position(5),
            current
        ));
        assert!(!queue_target_matches_current(
            QueueItemTarget::Position(4),
            None
        ));
    }
}
