# Matchbox Audio Implementation Plan

This plan tracks implementation work for Matchbox Audio. Requirements live in
`docs/requirements.md`; this document is for sequencing and scheduling.

The main scheduling bias is to get a deployable Pi image and remote development
loop working early. Features should be tested on the target device as soon as
there is enough system scaffolding to do so.

## Phase 0: Repository and Minimal Rust Skeleton

- [x] Create Rust workspace.
- [x] Add crates:
  - [x] `mba-protocol`
  - [x] `mba-player`
  - [x] `mba-cli`
- [x] Add repository-level `.gitignore`.
- [x] Adopt initial Rust stack:
  - [x] `tokio`
  - [x] `axum`
  - [x] `clap`
  - [x] `serde` / `serde_json`
  - [x] `tracing`
- [x] Add basic developer commands for format, lint, test, and build.
- [x] Add initial README with local build/run basics.
- [x] Implement `mba-player` as a minimal daemon with:
  - [x] config loading
  - [x] logging
  - [x] `GET /api/v1/status`
  - [x] static placeholder response identifying version/build info
  - [x] HTTP/WebSocket port `8090`
  - [x] minimal placeholder web page served from the daemon
- [x] Implement `mba-cli status` against local `mba-player`.
- [x] Add protocol serialization tests.

## Phase 1: Early Yocto Image and Remote Deploy Loop

- [x] Add the pi-base layer/helpers from the `sparkle-duck-shared` repository.
- [x] Create Matchbox Audio Yocto layer.
- [x] Create image recipe using A/B root filesystems and shared `/data`.
- [x] Document the 64 GB card partition assumption:
  - [x] boot partition
  - [x] root filesystem A
  - [x] root filesystem B
  - [x] ext4 `/data` partition using the remaining space
- [x] Package minimal `mba-player`.
- [x] Package minimal `mba-cli`.
- [x] Add `mba-player.service`.
- [x] Include base networking, SSH, Avahi, NetworkManager, and BlueZ.
- [x] Create checked-in `config/hotspot.local.example.json`.
- [x] Define gitignored `config/hotspot.local.json`.
- [x] Create initial flash script.
- [x] Create remote deploy/update script that reuses pi-base helpers.
- [x] Flash the Pi and verify:
  - [x] SSH works
  - [x] `mba-player.service` starts
  - [x] `mba-cli status` works over SSH
  - [x] `/data` mounts
  - [x] A/B slot status is readable

Phase 1 target note: the verified Raspberry Pi is reachable at
`matchbox-audio.local` as `matchbox`. Remote A/B update and smoke verification
passed with the device booted from slot `b` (`/dev/mmcblk0p3`). During that run,
the rootfs flash and SSH-key injection succeeded, but the final boot-slot switch
had to be completed directly on the FAT boot partition because `/boot` on the
running rootfs was not the mounted boot partition. Follow-up fix: add a
Matchbox-specific `base-files` fstab override so future images mount
`/dev/mmcblk0p1` at `/boot`, matching the dirtsim update pattern.

Follow-up status: the `/boot` fstab override is implemented and verified by
remote A/B updates on `matchbox-audio.local`. The update flow also ensures the
Pirate Audio boot config lines are present before rebooting into a new rootfs.

## Phase 2: Hotspot and Target Networking

- [x] Define mutually exclusive network modes:
  - [x] car mode as WPA2 hotspot
  - [x] home mode as Wi-Fi client
  - [x] no AP/client simultaneous operation for MVP
- [x] Implement flash-time hotspot config loading.
- [x] Generate NetworkManager hotspot profile.
- [x] Configure WPA2 SSID/password.
- [x] Ensure hotspot starts by default in car mode.
- [x] Verify web/API access over hotspot.
- [x] Verify SSH access over hotspot.
- [x] Add `rsync` to the image and verify `rsync` access over hotspot.
- [x] Show hotspot status in `mba-cli status`.
- [x] Record target-network troubleshooting notes.

Phase 2 status note: `/usr/bin/mba-network-mode` owns mutually exclusive
`home`, `car`, `toggle`, `restore`, and `status` operations. `car` mode brings
up the NetworkManager shared WPA2 hotspot profile `matchbox-car-hotspot` with
SSID `matchbox-audio`; `home` mode tears it down and restores the saved home Wi-Fi
connection. The standalone `dnsmasq.service` is masked in the image because it
conflicts with NetworkManager shared hotspot mode. On-device verification on
`matchbox-audio.local` confirmed Y-button long press activates the hotspot, and
the Phase 2 status update was A/B deployed to slot `b` on May 9, 2026 with
`mba-cli status` reporting the current network mode, active connection, IPv4
address, hotspot SSID, and hotspot password.

Hotspot client verification on May 9, 2026 used the workstation Wi-Fi joined to
`matchbox-audio` while Ethernet stayed connected as the fallback path. From the
hotspot client, `10.42.0.1` responded to ping, `/` rendered the web page with
`Network: car`, `/api/v1/status` reported `mode=car`, and SSH to
`matchbox@10.42.0.1` ran `mba-cli status`. Follow-up verification after adding
`rsync` to the image confirmed `rsync` over hotspot into
`/data/music/_sync-test`. The host-side `sync_music.sh` helper defaults to
syncing `~/Music` to `/data/music`, and a small test sync verified the helper
path to `/data/music/_sync-script-test`.

Phase 2 completion note: `yocto/scripts/flash.mjs` now reads the gitignored
`config/hotspot.local.json` file at flash time and injects
`/data/matchbox-audio/network/hotspot.env` after any `/data` restore so local
hotspot settings win. The `mba-network-mode-restore.service` boot unit restores
the saved mode from `/data/matchbox-audio/network/mode` and defaults to car mode
when no saved mode exists. On May 9, 2026, the rebuilt image was A/B deployed to
slot `a`; smoke tests passed in saved home mode, the restore unit exited
successfully, `rsync 3.2.7` was present, and a no-mode simulation switched the
device to car mode with `10.42.0.1` reachable before restoring the device to
home mode.

## Phase 2.5: Service Users and Permission Hardening

- [x] Define target user and group model before adding MPD/library write paths:
  - [x] `matchbox` as SSH/deploy/admin user
  - [x] `mba-player` as unprivileged app daemon user
  - [x] `mba-device` as root or hardware-capable service user
  - [ ] `mpd` as playback daemon user
- [x] Replace broad `matchbox ALL=(ALL) NOPASSWD: ALL` sudo with an allowlist.
- [x] Allow `matchbox` only the deployment/admin commands it needs:
  - [x] A/B update helpers
  - [x] reboot/poweroff
  - [x] selected `systemctl` service operations
  - [x] journal read access through the `systemd-journal` group
  - [x] network-mode helper commands
- [x] Decide whether `mba-player` should call privileged helpers directly:
  - [x] prefer no sudo for normal status reads
  - [x] keep `mba-player` on unprivileged `/usr/bin/mba-network-mode status`
- [x] Define `/data` ownership and modes:
  - [x] `/data/music` writable by SSH/admin workflow
  - [x] `/data/matchbox-audio/state` writable by Matchbox app services
  - [ ] `/data/mpd` writable by MPD
  - [x] hotspot/network state readable only as needed
- [x] Stop exposing hotspot password in unauthenticated status by default, or
  gate it behind a local/admin-only path.
- [x] Add minimal systemd hardening for services where practical.
- [x] Verify remote deploy, `mba-cli status`, button network switching, and
  library access after permission tightening. MPD is deferred to Phase 3.

Phase 2.5 rationale: the Phase 1/2 image intentionally uses a development
posture: SSH key login as `matchbox`, a local-console recovery password, and
full passwordless sudo for fast bring-up. Before adding MPD, library browsing,
and write-heavy app state, tighten this into explicit service boundaries so
network control, hardware access, playback state, and user music do not all
share the same privilege level.

Phase 2.5 verification note: on May 9, 2026, the hardened image was deployed to
`matchbox-audio.local` and `npm run hardening` passed against the device. The
suite asserts service activation, the public `/api/v1/status` payload omits
`hotspot_password`, `mba-network-mode status` is reachable without sudo and
also omits the password, `mba-network-mode display-status` is denied for
`matchbox` (the secret file is unreadable), `/data` directory ownership and
modes match the Phase 2.5 layout
(`/data/matchbox-audio` `root:root 0711`, `/data/matchbox-audio/state`
`mba-player:matchbox-audio 0750`, `/data/matchbox-audio/network`
`root:matchbox-audio 0750`, `/data/matchbox-audio/update` and `/data/music`
`matchbox:matchbox`), the sudo allowlist denies plain `sudo`, shell `sudo`,
shadow reads, and `sudo journalctl` while still allowing
`mba-boot-config ensure-pirate-audio` and the network-mode commands, and the
`matchbox` user reads the journal through `systemd-journal` group membership.
Button-driven network switching is wired through
`sudo mba-network-mode toggle`, which is in the allowlist and was sanity
checked via `sudo -n -l`. MPD coverage is deferred to Phase 3 once the daemon
lands.

## Phase 3: MPD on Target

- [ ] Add MPD to the Yocto image.
- [ ] Configure MPD to bind locally only.
- [ ] Use `127.0.0.1:6600` initially.
- [ ] Keep Unix socket support as an optional later refinement.
- [ ] Configure MPD music, database, playlist, and state paths under `/data`.
- [ ] Add or configure `mpd.service`.
- [ ] Configure minimal PIM483 I2S line-out audio.
- [ ] Keep ALSA/DAC output at fixed line level.
- [ ] Configure MPD software volume with maximum and startup volume caps.
- [ ] Verify clean MPD playback through PIM483 line-out on the Pi.
- [ ] Select Rust MPD client crate or implement minimal MPD protocol client.
- [ ] Implement `mba-player` MPD connection management.
- [ ] Implement playback commands:
  - [ ] play
  - [ ] pause
  - [ ] toggle
  - [ ] stop
  - [ ] next
  - [ ] previous
  - [ ] seek
  - [ ] volume
- [ ] Add CLI support for playback commands.
- [ ] Verify commands remotely with `mba-cli`.
- [ ] Implement status polling or MPD idle/event integration.

## Phase 4: Filesystem Library Browsing and Queueing

- [ ] Implement safe path handling under `/data/music`.
- [ ] Do not follow symlinks.
- [ ] Ignore hidden files and directories by default.
- [ ] Use case-insensitive audio extension filtering.
- [ ] Implement `GET /api/v1/library/list?path=...`.
- [ ] Implement path search.
- [ ] Implement queue by file.
- [ ] Implement queue by directory.
- [ ] Define stable recursive directory ordering:
  - [ ] depth-first traversal
  - [ ] directories before files
  - [ ] case-insensitive name sort with original name as tie-breaker
- [ ] Filter supported audio formats:
  - [ ] Ogg
  - [ ] MP3
  - [ ] FLAC
- [ ] Ignore non-audio files for playback operations.
- [ ] Keep Phase 4 browsing and queueing filesystem/path-only.
- [ ] Add CLI commands:
  - [ ] `mba-cli list`
  - [ ] `mba-cli search`
  - [ ] `mba-cli enqueue-file`
  - [ ] `mba-cli enqueue-dir`
- [ ] Test with files synced by SSH/`rsync` to `/data/music`.
- [ ] Add filesystem traversal tests.

## Phase 5: WebSocket Events and API Hardening

- [ ] Define shared command, response, event, and error envelopes.
- [ ] Implement WebSocket command handling.
- [ ] Implement WebSocket event broadcast.
- [ ] Emit playback state changed events.
- [ ] Emit track changed events.
- [ ] Emit queue changed events.
- [ ] Emit errors and warnings.
- [ ] Add API tests for request/response behavior.
- [ ] Add CLI event watch command.
- [ ] Verify event behavior remotely on the Pi.

## Phase 6: Minimal Web App on Device

- [ ] Choose web app stack.
- [ ] Serve static web app from `mba-player`.
- [ ] Implement playback status and controls.
- [ ] Implement directory/file browser.
- [ ] Implement file and directory enqueue actions.
- [ ] Implement queue view.
- [ ] Implement path search.
- [ ] Verify the app works without internet access.
- [ ] Verify the app over the Pi hotspot.

## Phase 7: Pirate Audio Display and Button Bring-Up

- [x] Configure PIM483 ST7789 display.
- [x] Configure PIM483 buttons.
- [ ] Implement button handling:
  - [ ] play/pause
  - [ ] previous
  - [ ] next
  - [x] fourth button placeholder/configurable action
- [ ] Implement compact display states:
  - [ ] booting
  - [ ] hotspot ready
  - [ ] now playing
  - [ ] paused/stopped
  - [ ] scanning
  - [ ] error
- [ ] Check for vehicle noise and document whether a ground-loop isolator is
  needed.

Phase 7 status note: `mba-device.service` now drives the Pirate Audio ST7789
display over SPI0 CE1 and monitors the fourth button GPIO candidates 20 and 24.
A long press on the fourth button runs `/usr/bin/mba-network-mode toggle`; a
short press only prompts the hold action on the display. Remote verification on
`matchbox-audio.local` confirms the service is active, SPI devices exist, and
display refreshes no longer report write failures. Physical Y-button validation
confirmed a long press switches `wlan0` from home Wi-Fi to the
`matchbox-audio` hotspot in car mode. The play/pause, previous, and next
buttons are intentionally still unbound until playback control exists; for now
they emit short/long press logs so the physical buttons can be validated.

## Phase 8: Metadata and Artwork Cache

- [ ] Design SQLite schema for supplemental metadata and artwork cache.
- [ ] Implement library scan job.
- [ ] Extract baseline metadata.
- [ ] Detect folder artwork.
- [ ] Detect or extract embedded artwork.
- [ ] Generate resized artwork for web/display use.
- [ ] Store cache under `/data/matchbox-audio`.
- [ ] Expose metadata/artwork through API.
- [ ] Keep playback controls responsive during scans.
- [ ] Add scan progress events.
- [ ] Test with a large library sample on the Pi.

## Phase 9: A/B Update and Data Persistence Validation

- [ ] Verify initial flash preserves configurable hotspot setup.
- [ ] Verify `/data/music` survives full reflash.
- [ ] Verify `/data/matchbox-audio` survives full reflash.
- [ ] Verify remote A/B update writes inactive rootfs.
- [ ] Verify reboot into updated slot.
- [ ] Verify rollback path.
- [ ] Document normal update workflow.

## Phase 10: Reliability and Field Testing

- [ ] Test abrupt power loss during playback.
- [ ] Test abrupt power loss during library scan.
- [ ] Test boot auto-resume.
- [ ] Test large library behavior with 30 GB or more of music.
- [ ] Test long-running playback.
- [ ] Test hotspot startup after cold boot.
- [ ] Test SSH/`rsync` sync followed by rescan.
- [ ] Review exposed services on the hotspot.
- [ ] Capture known issues and operational notes.

## Deferred Work

- [ ] BLE control or provisioning.
- [ ] Bluetooth audio sink mode.
- [ ] USB mass-storage maintenance mode.
