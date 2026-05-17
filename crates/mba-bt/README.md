# Matchbox Bluetooth Request Router

`mba-bt` is the local-first Bluetooth control daemon crate. The core request
handling stays independent from BlueZ and Android so it can be tested without
Bluetooth hardware.

Current pieces:

- `RequestRouter` accepts complete `ProtocolMessage` requests and returns
  protocol responses/events.
- `HttpPlayerBackend` queries `mba-player` for real `system.snapshot`
  responses.
- `FakePlayerBackend` supplies deterministic `system.snapshot` responses for
  tests and fake-player local smoke runs.
- `SessionGate` enforces one active client and returns structured `busy`
  errors for later connection code.
- `FrameDecoder` and `encode_frame` provide a local length-prefixed development
  transport.
- `InMemoryBleTransport` proves BLE chunks can reassemble into the same router
  path used by framed local transports.
- The BLE-local GATT mode registers the Matchbox service through BlueZ and uses
  `mba-player` over local HTTP by default.
- BLE-local treats TX notification subscription as the active app session,
  gates RX writes by Bluetooth address, and releases the session when
  notifications close.

Run the local stdio exerciser:

```sh
cargo run -p mba-bt -- --stdio
```

The stdio mode reads and writes the same local frame format documented in the
Android app plan:

```text
uint32_le json_length
utf8_json_payload
```

This is intended for small developer tools and tests. Real BLE GATT
registration starts with the BLE-local mode:

```sh
cargo run -p mba-bt -- --ble-local
```

`--ble-local` registers the Matchbox GATT service through BlueZ, advertises as
`Matchbox Audio`, accepts RX writes, routes complete messages through
`RequestRouter`, and sends TX notifications. By default, `system.snapshot`
queries `mba-player` at `http://127.0.0.1:8090`. Use `--player-server <url>`
to point at another daemon or `--fake-player` for deterministic local BLE
smoke tests without MPD.

The active app session starts when a phone subscribes to the TX notification
characteristic. That gives `mba-bt` a response path. A second subscriber gets a
structured `busy` response when possible, and RX writes from inactive Bluetooth
addresses are rejected.

BLE-local opens a persistent state store before registering with BlueZ. The
packaged service uses `/data/matchbox-audio/bt`; local developer runs can pass
`--state-dir /tmp/mba-bt/state` or `--no-state-dir`. The store currently creates
`state.json` and a `clients/` directory for the later trusted-client records.

BLE-local also serves a local admin socket for CLI inspection:

```sh
cargo run -p mba-cli -- bt status
```

The packaged service uses `/run/mba-bt/control.sock`. For a local developer run,
use `--control-socket /tmp/mba-bt/control.sock` on `mba-bt` and
`--socket /tmp/mba-bt/control.sock` on `mba-cli`. Use `--no-control-socket` to
disable the socket. It reports adapter, advertising, pairing, busy,
active-client, and RX/TX counter state without going through BLE.

The same socket controls the runtime pairing window:

```sh
cargo run -p mba-cli -- bt --socket /tmp/mba-bt/control.sock pairing start --timeout 120
cargo run -p mba-cli -- bt --socket /tmp/mba-bt/control.sock pairing stop
```

Trusted-client records can be inspected and removed over the same socket:

```sh
cargo run -p mba-cli -- bt --socket /tmp/mba-bt/control.sock clients
cargo run -p mba-cli -- bt --socket /tmp/mba-bt/control.sock forget <client-id>
```

Pairing mode is intentionally not persisted. Rebooting `mba-bt` closes the
window, but trusted-client records added by later phases will live in the state
store.
