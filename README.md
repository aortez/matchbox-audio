# Matchbox Audio

Matchbox Audio is an in-car local music player for Raspberry Pi hardware. The
first target is a Raspberry Pi Zero 2 W with a Pimoroni Pirate Audio Line Out
board, local music storage, MPD playback, a Wi-Fi hotspot, and a browser-based
control surface.

Design notes live in:

- `docs/requirements.md`
- `docs/implementation-plan.md`

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
  "bind": "127.0.0.1:8090"
}
```

Command-line `--bind` overrides the config file.

