# Matchbox Audio

Matchbox Audio is an in-car local music player, using a Raspberry Pi Zero 2 W with a Pimoroni Pirate Audio Line Out
board and local music storage.

The first target is MPD playback, a Wi-Fi hotspot, and a webapp ui.  This
is reasonably useable now.

The second target is an Android app, see docs/android-app-design.md and
docs/android-app-plan.md.  This is WIP.

Design notes live in /docs.

## Workspace

The Rust workspace contains three initial crates:

- `mba-protocol`: shared API and serialization types
- `mba-player`: long-running daemon and HTTP API server
- `mba-cli`: command-line client for local or remote control

## Developer Commands

```sh
make fmt
make lint
make test
make build
make yocto-update
```

The Makefile uses the `CARGO` variable, so a non-default toolchain can be used
with:

```sh
make CARGO="$HOME/.cargo/bin/cargo" test
```

## Local Run

Start the daemon:

```sh
cargo run -p mba-player
```

The daemon listens on `0.0.0.0:8090` by default and serves:

- `GET /`
- `GET /api/v1/status`

Query it from another shell:

```sh
cargo run -p mba-cli -- status
```

Use a different daemon address with:

```sh
cargo run -p mba-cli -- --server http://127.0.0.1:8090 status
```

## Player Configuration

`mba-player` can load a small JSON config file with `--config`:

```json
{
  "bind": "127.0.0.1:8090",
  "music_root": "/data/music"
}
```

Command-line `--bind`, `--mpd-addr`, and `--music-root` override the config
file.

## Remote Device Update

After the device has been flashed once and is reachable over SSH:

```sh
./update.sh --skip-build --smoke
```

Use `--target <host>` for a different device. The wrapper performs a full Yocto
A/B rootfs update and preserves `/data`.
