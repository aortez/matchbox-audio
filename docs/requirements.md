# Matchbox Audio Requirements

## Purpose

Matchbox Audio is an in-car local music player for an older vehicle with an
auxiliary line input. The target first installation is a 2009 Subaru WRX. The
system should behave like a small embedded appliance: it boots reliably, plays
music stored locally, exposes simple controls, and does not depend on internet
access while driving.

## Goals

- Play music from local storage on the device.
- Output clean line-level stereo audio suitable for a vehicle aux input.
- Present a Wi-Fi hotspot for phone-based control in the car.
- Serve a client-side web app for browsing and playback control.
- Provide a command-line tool for setup, diagnostics, and scripted control.
- Use the filesystem layout as the primary music browsing model.
- Use MPD as the playback engine unless a clear limitation appears.
- Keep project-specific behavior in Matchbox Audio rather than exposing MPD as
  the primary public interface.
- Use Raspberry Pi Linux infrastructure where it reduces appliance work:
  systemd, NetworkManager, BlueZ, Avahi, persistent data, and Yocto packaging.
- Preserve music, configuration, and user state across image updates.

## Non-Goals

- Streaming from internet services.
- A large native on-device browsing UI.
- Direct speaker amplification.
- Depending on a phone app store deployment for the primary control surface.
- Building a custom audio decoder/player before MPD has been proven
  insufficient.
- Artist, album, genre, or other tag-taxonomy views as first-version primary
  navigation.
- Uploading music through the web app.
- Bluetooth audio sink support in the first version.
- USB mass-storage export in the first version.

## Target Hardware

Initial target:

- Raspberry Pi Zero 2 W.
- Pimoroni Pirate Audio Line Out PIM483.
- microSD storage for OS, app state, and music.
- Vehicle aux input as the audio sink.
- Automotive-safe 12V-to-5V power supply.

Hardware assumptions:

- The PIM483 provides line-level audio output over I2S.
- The PIM483 display is used for status and now-playing information.
- The PIM483 buttons are used for blind controls such as play/pause, previous,
  next, and a fourth configurable action.
- A ground-loop isolator may be needed if vehicle audio noise appears.

## Software Components

- `mba-player`: long-running daemon and main application service.
- `mba-cli`: command-line client for local or remote control.
- `mba-protocol`: shared Rust crate for API types, commands, responses, events,
  and errors.
- Web app: static client-side application served by `mba-player`.
- MPD: local playback backend managed by the system image.
- Yocto layer: system image integration, services, dependencies, and defaults.

Initial implementation choices:

- Rust async/runtime stack: `tokio`.
- HTTP/WebSocket stack: `axum`.
- CLI argument parsing: `clap`.
- Serialization: `serde` and `serde_json`.
- Logging: `tracing`.
- `mba-player` HTTP/WebSocket port: `8090`.
- MPD local interface: `127.0.0.1:6600` initially, with a Unix socket as a
  later option if it simplifies service isolation.
- Hotspot example config path: `config/hotspot.local.example.json`.
- Hotspot local config path: `config/hotspot.local.json`.

## High-Level Architecture

`mba-player` owns product behavior. It wraps MPD and exposes Matchbox Audio APIs
to clients. Other interfaces should translate into the same internal command
model.

Control flow:

```text
web app      -> mba-player API -> MPD
mba-cli      -> mba-player API -> MPD
PIM483 keys  -> mba-player     -> MPD
BLE later    -> mba-player API -> MPD
```

MPD should listen only on a local interface by default. The public API on the
car hotspot should be Matchbox Audio's API, not raw MPD.

## Storage Layout

Expected persistent paths:

- `/data/music`: local music library.
- `/data/matchbox-audio`: application state, configuration, indexes, and logs.
- `/data/mpd`: MPD database, playlists, and playback state if practical.

The expected music library will eventually be 30 GB or more on a 64 GB card.
Indexing, artwork caches, logs, and update data should be sized with that
constraint in mind.

The system should tolerate abrupt power loss. Writes during playback should be
bounded and intentional.

## Music Library Requirements

- Primary formats are Ogg, MP3, and FLAC.
- The user-organized filesystem layout under `/data/music` is the primary
  library organization.
- Browsing should expose directories and playable files.
- Queueing should work by individual file path.
- Queueing should work by directory path.
- Directory queueing should recursively add supported audio files in a stable,
  predictable order.
- Directory entries that are not supported audio files or useful artwork should
  be ignored by playback operations.
- Music sync is external to the web app.
- SSH and `rsync` are the preferred first sync mechanisms.
- The device should support library rescans after external sync.
- The player should remain responsive while rescans are running.

## Metadata and Artwork Requirements

The filesystem path is the primary browsing and queueing identity. Metadata and
artwork improve display quality, search, and now-playing context, but should not
replace the directory/file model as the first-version navigation structure.

Baseline metadata:

- track title
- artist
- album
- album artist where available
- track number and disc number where available
- duration
- source path or stable track ID
- file format

Artwork sources to consider:

- embedded artwork in audio files
- folder artwork such as `cover.jpg`, `folder.jpg`, or `front.jpg`
- MPD-provided artwork APIs if they are sufficient on the target MPD version
- a Matchbox Audio-managed artwork cache under `/data/matchbox-audio`

Metadata and artwork indexing should use a hybrid approach:

- MPD owns playback and its normal music database.
- Matchbox Audio maintains a supplemental SQLite/cache layer for filesystem
  browsing, path search, stable IDs, display metadata, and resized artwork.
- Cached metadata and artwork should live under `/data/matchbox-audio`.

## Playback Requirements

- Play Ogg, MP3, FLAC, and other common local audio formats supported by MPD.
- Support play, pause, stop, next, previous, seek, volume, queue clear, enqueue,
  shuffle, and repeat controls.
- Support scanning or rescanning the local library.
- Preserve enough state to resume the previous queue and track after reboot.
- Resume playback automatically after power-on.
- Make boot resume behavior configurable so automatic resume can be disabled
  later if needed.

## Network Requirements

Primary in-car mode:

- Create a Wi-Fi hotspot by default.
- Secure the hotspot with WPA2.
- Configure hotspot SSID and password during flashing from a local JSON config
  file.
- Serve the web app from the device.
- Support local discovery by hostname when practical.
- Avoid requiring internet access.

Future or optional home mode:

- Join a home Wi-Fi network for music sync, updates, or administration.
- Support SSH/`rsync` music sync over the home network or hotspot.
- Reuse existing BLE Improv WiFi provisioning infrastructure if it fits the
  final image.

## API Requirements

The API should support both request/response control and live event updates.

HTTP should handle simple resource queries and one-shot actions:

- `GET /api/v1/status`
- `GET /api/v1/library/list?path=...`
- `GET /api/v1/library/search?q=...`
- `GET /api/v1/queue`
- `POST /api/v1/queue/files`
- `POST /api/v1/queue/directories`
- `POST /api/v1/playback/play`
- `POST /api/v1/playback/pause`
- `POST /api/v1/playback/next`
- `POST /api/v1/library/rescan`

WebSocket should handle richer command/event flows:

- playback state changed
- track changed
- queue changed
- library scan progress
- hardware button events
- errors and warnings

Messages should use typed JSON envelopes with correlation IDs for commands and
monotonic sequence numbers for events.

## CLI Requirements

`mba-cli` should be useful from SSH, serial console, and development machines.

Initial commands:

- `mba-cli status`
- `mba-cli play`
- `mba-cli pause`
- `mba-cli toggle`
- `mba-cli next`
- `mba-cli previous`
- `mba-cli seek`
- `mba-cli volume`
- `mba-cli queue`
- `mba-cli list`
- `mba-cli enqueue-file`
- `mba-cli enqueue-dir`
- `mba-cli search`
- `mba-cli scan`
- `mba-cli events`
- `mba-cli logs`

The CLI should default to the local daemon when run on-device and allow an
address override for remote control during development.

## Web App Requirements

The web app is the primary rich control surface.

Initial capabilities:

- Include a minimal placeholder page in the first deployable build so static
  file serving is proven before the full web app exists.
- Show current playback state and now-playing metadata.
- Browse the local library as directories and files.
- Search paths and display metadata.
- Add individual files or directories to the queue.
- Control playback and volume.
- Show queue contents.
- Trigger library rescan.
- Show basic device/network status.
- Do not support uploading music.

The web app must work without internet access once served from the player.

## Display and Button Requirements

The PIM483 display should show compact appliance status:

- booting
- hotspot ready
- connected clients or web URL hint
- scanning library
- now playing
- paused/stopped
- playback or MPD error

Buttons should be usable without looking at a phone:

- play/pause
- previous
- next
- configurable action

Open decision:

- The fourth button behavior: favorite, shuffle toggle, hotspot info, or safe
  shutdown.

## Bluetooth Requirements

BLE is optional for the first version.

Potential BLE uses:

- simple transport controls
- provisioning home Wi-Fi
- exposing now-playing status to a custom Android app

Bluetooth audio sink support is a later-version feature. It is separate from
BLE control/provisioning and should not block the first local-library player.

BLE should not be the primary library browsing interface.

## Future Maintenance Interfaces

USB mass-storage export is out of scope for the first version. If added later,
it should be implemented as an explicit maintenance mode rather than normal
player operation.

Possible approaches:

- export a dedicated music partition over the Pi Zero 2 W USB OTG port after
  stopping playback and unmounting it locally
- expose a file-oriented protocol such as MTP instead of a raw block device
- continue to prefer SSH/`rsync` and avoid USB export entirely

## Image and Deployment Requirements

The device image should be built with Yocto.

The image must use A/B root filesystem partitioning with a shared persistent
`/data` partition. System updates should write the inactive root filesystem and
preserve `/data` across reflashes, updates, and rollbacks.

Matchbox Audio should reuse the pi-base/sparkle-duck-shared A/B update
machinery. Project-specific update scripts may wrap those shared helpers, but
`mba-player` and the web app do not need to expose update controls in the first
version.

Flashing should support a local hotspot configuration file. The repository
should include `config/hotspot.local.example.json`, which users copy to
`config/hotspot.local.json` and edit with their own SSID and password.
`config/hotspot.local.json` must be gitignored.

Example shape:

```json
{
  "hotspot": {
    "ssid": "matchbox-audio",
    "password": "change-me"
  }
}
```

Relevant base infrastructure:

- A/B update layout.
- Persistent `/data` partition.
- NetworkManager.
- Avahi.
- BlueZ.
- systemd services.
- flash-time SSH and hostname setup.

System services:

- `mpd.service`
- `mba-player.service`
- hotspot/network setup service or NetworkManager profile

## Reliability Requirements

- Recover cleanly from sudden power loss.
- Start services automatically on boot.
- Keep playback controls responsive while library scans are running.
- Keep the web app available without internet access.
- Log enough information to debug audio, MPD, storage, hotspot, and button
  issues.
- Avoid exposing unnecessary services on the hotspot.

## Security Requirements

- Do not expose raw MPD to the hotspot by default.
- Prefer local-only MPD binding.
- Use WPA2 for the car hotspot.
- Avoid storing secrets outside persistent protected locations.
- Keep the real hotspot configuration file out of git.
- Treat the WPA2-protected car hotspot as a local trusted network for the first
  version, but keep API boundaries explicit enough to add authentication later.

## Open Questions

- What should the fourth hardware button do?
- What is the implementation scope for later AirPlay or Bluetooth audio sink
  support?
