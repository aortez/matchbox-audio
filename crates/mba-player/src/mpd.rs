use std::{net::SocketAddr, time::Duration};

use mba_protocol::{
    basename, LibraryDirectory, LibraryListing, LibraryTrack, PlaybackInfo, PlaybackState,
    TrackInfo,
};
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
    Seek { seconds: f64 },
    SetVolume { level: u8 },
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
    ListLibrary {
        path: String,
        reply: oneshot::Sender<Result<LibraryListing, MpdError>>,
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
            Request::ListLibrary { reply, .. } => {
                let _ = reply.send(Err(error));
            }
        }
    }
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

    pub async fn rescan(&self) -> Result<u64, MpdError> {
        let (tx, rx) = oneshot::channel();
        self.commands
            .send(Request::Rescan { reply: tx })
            .await
            .map_err(|_| MpdError::ChannelClosed)?;
        rx.await.map_err(|_| MpdError::ChannelClosed)?
    }

    pub async fn list_library(&self, path: String) -> Result<LibraryListing, MpdError> {
        let (tx, rx) = oneshot::channel();
        self.commands
            .send(Request::ListLibrary { path, reply: tx })
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
        Request::ListLibrary { path, reply } => handle_list_library(client, path, reply).await,
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

async fn handle_list_library(
    client: &Client,
    path: String,
    reply: oneshot::Sender<Result<LibraryListing, MpdError>>,
) -> bool {
    let mut command = RawCommand::new("lsinfo");
    if !path.is_empty() {
        if let Err(error) = command.add_argument::<&str>(path.as_str()) {
            let message = error.to_string();
            warn!(%message, "lsinfo argument rejected");
            let _ = reply.send(Err(MpdError::Command(message)));
            return true;
        }
    }
    match client.raw_command(command).await {
        Ok(frame) => {
            let listing = parse_listing(path, frame.fields());
            let _ = reply.send(Ok(listing));
        }
        Err(error) => {
            let message = error.to_string();
            warn!(%message, "MPD lsinfo failed");
            let _ = reply.send(Err(MpdError::Command(message)));
        }
    }
    true
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
    let track = current.map(|entry| {
        let song = entry.song;
        TrackInfo {
            uri: song.url.clone(),
            title: song.title().map(str::to_string),
            artist: song.artists().first().cloned(),
            album: song.album().map(str::to_string),
            duration_s: song.duration.map(|d| d.as_secs() as u32),
            elapsed_s: status.elapsed.map(|d| d.as_secs() as u32),
        }
    });
    PlaybackInfo {
        state,
        volume: status.volume,
        queue_length: status.playlist_length as u32,
        track,
    }
}

struct PendingTrack {
    uri: String,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration_s: Option<u32>,
}

impl PendingTrack {
    fn new(uri: String) -> Self {
        Self {
            uri,
            title: None,
            artist: None,
            album: None,
            duration_s: None,
        }
    }

    fn into_track(self) -> LibraryTrack {
        LibraryTrack {
            name: basename(&self.uri).to_string(),
            uri: self.uri,
            title: self.title,
            artist: self.artist,
            album: self.album,
            duration_s: self.duration_s,
        }
    }
}

fn parse_listing<'a, I>(path: String, fields: I) -> LibraryListing
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut directories = Vec::new();
    let mut tracks = Vec::new();
    let mut pending: Option<PendingTrack> = None;
    let mut in_playlist = false;

    for (key, value) in fields {
        match key {
            "directory" => {
                if let Some(track) = pending.take() {
                    tracks.push(track.into_track());
                }
                in_playlist = false;
                directories.push(LibraryDirectory {
                    name: basename(value).to_string(),
                    path: value.to_string(),
                });
            }
            "file" => {
                if let Some(track) = pending.take() {
                    tracks.push(track.into_track());
                }
                in_playlist = false;
                pending = Some(PendingTrack::new(value.to_string()));
            }
            "playlist" => {
                if let Some(track) = pending.take() {
                    tracks.push(track.into_track());
                }
                in_playlist = true;
            }
            "Title" => {
                if let Some(track) = pending.as_mut() {
                    track.title = Some(value.to_string());
                }
            }
            "Artist" => {
                if let Some(track) = pending.as_mut() {
                    track.artist = Some(value.to_string());
                }
            }
            "Album" => {
                if let Some(track) = pending.as_mut() {
                    track.album = Some(value.to_string());
                }
            }
            "Time" | "duration" => {
                if let Some(track) = pending.as_mut() {
                    if let Ok(secs) = value.parse::<f64>() {
                        track.duration_s = Some(secs.round().max(0.0) as u32);
                    }
                }
            }
            _ => {
                let _ = in_playlist;
            }
        }
    }
    if let Some(track) = pending.take() {
        tracks.push(track.into_track());
    }

    LibraryListing {
        path,
        directories,
        tracks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_listing_empty_returns_empty_listing() {
        let listing = parse_listing(String::new(), [].iter().copied());
        assert_eq!(listing.path, "");
        assert!(listing.directories.is_empty());
        assert!(listing.tracks.is_empty());
    }

    #[test]
    fn parse_listing_groups_directories_and_files() {
        let fields: &[(&str, &str)] = &[
            ("directory", "Pink Floyd"),
            ("Last-Modified", "2024-01-01T00:00:00Z"),
            ("directory", "Radiohead"),
            ("Last-Modified", "2024-01-02T00:00:00Z"),
            ("file", "single.flac"),
            ("Title", "Single"),
            ("Artist", "Various"),
            ("Album", "Mixtape"),
            ("Time", "214"),
            ("duration", "213.876"),
            ("playlist", "legacy.m3u"),
            ("Last-Modified", "2024-02-01T00:00:00Z"),
        ];
        let listing = parse_listing(String::new(), fields.iter().copied());

        assert_eq!(listing.directories.len(), 2);
        assert_eq!(listing.directories[0].name, "Pink Floyd");
        assert_eq!(listing.directories[0].path, "Pink Floyd");
        assert_eq!(listing.directories[1].name, "Radiohead");

        assert_eq!(listing.tracks.len(), 1);
        let track = &listing.tracks[0];
        assert_eq!(track.uri, "single.flac");
        assert_eq!(track.name, "single.flac");
        assert_eq!(track.title.as_deref(), Some("Single"));
        assert_eq!(track.artist.as_deref(), Some("Various"));
        assert_eq!(track.album.as_deref(), Some("Mixtape"));
        assert_eq!(track.duration_s, Some(214));
    }

    #[test]
    fn parse_listing_uses_basename_for_nested_files() {
        let fields: &[(&str, &str)] = &[
            ("file", "Pink Floyd/Dark Side/01 Speak to Me.flac"),
            ("Title", "Speak to Me"),
        ];
        let listing = parse_listing("Pink Floyd/Dark Side".to_string(), fields.iter().copied());
        let track = &listing.tracks[0];
        assert_eq!(track.uri, "Pink Floyd/Dark Side/01 Speak to Me.flac");
        assert_eq!(track.name, "01 Speak to Me.flac");
    }
}
