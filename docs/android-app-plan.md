# Matchbox Audio Android App Plan

This plan sequences the Android app and Bluetooth bridge work. The architecture
and rationale live in `docs/android-app-design.md`; this document tracks
execution order, test gates, and deployment milestones.
The message passing stack is diagrammed in `docs/android-message-stack.md`.

The bias is local-first development: most protocol, bridge, and app behavior
should be testable without Bluetooth hardware. Real Pi and Android phone tests
are still required before calling the feature usable.

## Current Decisions

- Android uses BLE GATT as the primary control transport.
- `mba-bt` is a separate bridge daemon; `mba-player` remains the owner of
  playback, library, queue, and product state.
- `mba-bt` v1 allows one active app connection.
- A second active app connection receives a structured `busy` response.
- RFCOMM is a fallback transport only if BLE reliability or throughput is poor
  on the target phone.
- Pairing mode starts as a CLI-triggered flow before hardware-button UX.
- Android v1 does not include artwork.
- Android v1 does not include search.
- Android v1 does not include bulk library sync, music transfer, or large
  diagnostics over Bluetooth.
- The web app remains available for fallback, diagnostics, and admin workflows.
- Private Android deployment starts with local debug installs, then manually
  distributed signed APKs. Play Store and Firebase-style distribution are
  deferred until there are multiple testers.

## Workstreams

### Protocol Contract

Owns the stable client/device language shared by Rust, Android, and eventually
the web app.

- Request, response, event, and error envelopes.
- Transport-neutral message encoding and parsing.
- BLE chunking, reassembly, and max message size.
- Paged result contracts for library and queue responses.
- Protocol version negotiation.
- Capability reporting.
- JSON fixtures shared by Rust and Android tests.
- Compatibility policy for future protocol changes.

### Device Bridge

Owns Bluetooth exposure and translation into existing Matchbox behavior.

- New `mba-bt` Rust crate.
- Transport abstraction for in-memory, TCP/dev, stdin/stdout/dev, and BLE GATT.
- Fake `mba-player` backend for tests.
- Local protocol exerciser.
- BlueZ GATT service registration and BLE advertising.
- Pairing/trust state under `/data/matchbox-audio/bt`.
- Yocto packaging and `mba-bt.service`.

### Android App

Owns the native user experience.

- Kotlin + Jetpack Compose project under `android/`.
- Compose screens for now playing, library, queue, setup, and settings.
- View models and repository layer.
- Fake and scripted transports for local development.
- Real BLE GATT transport.
- Connection lifecycle and reconnect behavior.
- Signed debug/release APK build path.

### Testing and Tooling

Owns local confidence before hardware testing.

- Rust unit and integration tests.
- Android JVM tests.
- Android instrumented Compose tests with fake transport.
- Device smoke tests over SSH.
- Later hardware-in-loop checks with a real Pi and Android phone.

## Phase 0: Planning and Spikes

- [x] Write Android app design document.
- [x] Write Android app execution plan.
- [x] Identify existing pi-base BLE GATT precedent from the Improv Wi-Fi
  provisioner.
- [x] Define Matchbox BLE service UUID and characteristic layout.
- [x] Define initial BLE chunk envelope, max message size, and paging defaults.
- [x] Validate `bluer` GATT/advertising API support on the development host
  with `tools/ble-gatt-spike` on May 16, 2026.
- [x] Validate `bluer` GATT/advertising API support on the target image with
  `tools/ble-gatt-spike` on `matchbox-audio.local` on May 16, 2026.
- [x] Decide whether direct `zbus` BlueZ integration is needed: not for the
  initial GATT/advertising implementation unless later Android testing exposes
  a `bluer` gap.
- [x] Confirm Android target phone can scan, connect, subscribe to
  notifications, request MTU, and exchange a chunked `system.hello`.
  - [x] Add `tools/android-ble-smoke` client for the phone-side proof.
  - [x] Extend `tools/ble-gatt-spike` to reassemble chunked `system.hello` and
    send a chunked response.
  - [x] Build/install the smoke APK with Android Studio.
  - [x] Run against a Pixel 7 Pro and `matchbox-audio.local` on May 16, 2026:
    scan found `Matchbox Audio`, MTU 517 succeeded, Status read succeeded, TX
    notifications delivered a three-chunk `system.hello` response.
- [x] Decide first private APK distribution path after local installs:
  continue using Android Studio or `adb install` for development, then use a
  manually distributed signed APK when the app controls real playback. Keep the
  signing key outside git. Defer Play Store, Firebase App Distribution, and
  other hosted distribution until there are multiple testers.

Exit criteria:

- The BLE GATT approach is proven on the Pi or a clear fallback is chosen.
- The target Android phone can complete at least one manual BLE exchange.
- The first chunk/message limits are documented.
- The private APK path is decided.
- The project has enough confidence to add `mba-bt` to the workspace.

## Phase 1: Shared Protocol Fixtures

- [x] Add shared protocol fixture directory.
- [x] Define request, response, event, and error envelopes in `mba-protocol`.
- [x] Define logical message format and max message size.
- [x] Define BLE chunk format and reassembly rules.
- [x] Define paged library and queue response shapes.
- [x] Add Rust encode/decode tests for core fixtures.
- [x] Add Rust message/chunk parser tests:
  - [x] partial message
  - [x] missing chunk
  - [x] oversized message
  - [x] malformed JSON
  - [x] unknown method
  - [x] unsupported protocol version
- [x] Define initial method names:
  - [x] `system.hello`
  - [x] `system.snapshot`
  - [x] `events.subscribe`
  - [x] playback methods
  - [x] library list methods
  - [x] queue methods
- [x] Define stable error codes:
  - [x] `bad_request`
  - [x] `unauthorized`
  - [x] `auth_required`
  - [x] `busy`
  - [x] `not_found`
  - [x] `unsupported_method`
  - [x] `unsupported_version`
  - [x] `player_unavailable`
  - [x] `internal`

Exit criteria:

- `cargo test -p mba-protocol` validates the protocol fixtures.
- The envelope supports request/response and unsolicited events.
- BLE chunking can round-trip valid fixture messages and reject malformed or
  oversized messages.
- Fixture format is stable enough for Android tests to consume.

## Phase 2: `mba-bt` Local Request Router

- [x] Add `crates/mba-bt`.
- [x] Add `mba-bt` to the workspace.
- [x] Implement transport trait or equivalent message abstraction.
- [x] Implement in-memory transport tests.
- [x] Implement local framed protocol read/write for stdio dev tools.
- [x] Implement BLE chunk/reassembly transport tests.
- [x] Implement `system.hello`.
- [x] Implement `system.snapshot`.
- [x] Implement event subscription plumbing with fake events.
- [x] Implement one-active-client gate.
- [x] Implement structured `busy` response for a second client.
- [x] Add fake `mba-player` backend.
- [x] Add local protocol exerciser command.
- [x] Add tests for:
  - [x] connect/disconnect cleanup
  - [x] auth-required response
  - [x] busy response
  - [x] player unavailable response
  - [x] malformed chunk/message handling
  - [x] unsupported method response
  - [x] event delivery to subscribed client

Exit criteria:

- `cargo test -p mba-bt` passes without Bluetooth hardware.
- A local developer tool can send framed requests and receive framed responses.
- The same router logic can run through an in-memory BLE-chunked transport.
- The router can be tested against a fake player backend.

## Phase 3: BlueZ BLE GATT Integration

- [x] Add BlueZ BLE GATT implementation behind the transport boundary.
- [x] Register Matchbox GATT service with a stable service UUID.
- [x] Add read-only capabilities/status characteristic.
- [x] Add app-to-device write characteristic.
- [x] Add device-to-app notify characteristic.
- [x] Advertise useful device/service name and pairing-mode state.
- [x] Accept a manual BLE connection from the Android target phone.
- [x] Verify MTU request behavior on the Android target phone.
- [x] Verify chunked `system.hello` and `system.snapshot` over real BLE.
- [ ] Verify oversized response rejection or pagination behavior.
- [x] Add service logging for connection, disconnect, and protocol errors.
- [x] Add `mba-cli bt status`.
- [ ] Add `mba-cli bt pairing start --timeout <seconds>`.
- [ ] Add `mba-cli bt pairing stop`.
- [ ] Add `mba-cli bt clients`.
- [ ] Add `mba-cli bt forget <client>`.

Phase 3 first-slice status:

- `mba-bt --ble-local` registers the GATT service, advertises as
  `Matchbox Audio`, and routes RX/TX chunks through `RequestRouter` with
  `FakePlayerBackend`.
- The Android smoke app now sends `system.snapshot` after the `system.hello`
  response, so a phone-side run can validate both methods over the same BLE
  connection.
- Validated on May 16, 2026 PDT with a Pixel 7 Pro and
  `matchbox-audio.local`: Android negotiated MTU 517, read Status, and
  reassembled three-chunk responses for `system.hello` and `system.snapshot`.
- Android GATT writes are serialized by the platform. The smoke app waits for
  `onCharacteristicWrite` before starting the next request.

Phase 3 second-slice status:

- `mba-bt` treats a TX notification subscription as the active app session.
  That is the point where the phone has opened the response path.
- The active client is tracked by Bluetooth address, adapter, MTU, and an
  internal session token.
- RX writes are accepted only from the active client address. Other clients are
  rejected at the GATT write layer.
- A second TX subscriber receives a structured `busy` protocol response where
  possible, then its notification writer is dropped.
- When the TX notification session closes, `mba-bt` releases `SessionGate` and
  clears partial BLE reassembly state.
- Status now reports `busy`, active client details, RX chunk-write count, and
  TX chunk-send count.
- Validated on May 16, 2026 PDT with a Pixel 7 Pro and
  `matchbox-audio.local`: Status reported `busy=false` before subscription,
  the subscribed client address was allowed to send RX chunks, and force-stopping
  the app closed TX notifications and cleared the active BLE session.
- Android negotiated MTU 517. The BlueZ notification writer exposed 512 sendable
  bytes after bluer's safety workaround, which is still comfortably above the
  protocol target GATT value size of 244 bytes.

Phase 3 CLI status status:

- `mba-bt --ble-local` serves a local admin socket at
  `/tmp/mba-bt/control.sock`.
- `mba-cli bt status` sends `bt.status` over that socket and prints adapter,
  advertising, pairing, busy, active-client, and RX/TX counter state.
- This socket is the local admin plane for the daemon. It is separate from the
  Android-facing BLE GATT service and does not add any phone-visible BLE
  characteristics.
- The packaged socket path is `/run/mba-bt/control.sock`, created by
  `mba-bt.service` through systemd's runtime directory support. Local developer
  runs can still use `--control-socket /tmp/mba-bt/control.sock`.

Exit criteria:

- A real Android phone can connect to the Pi over BLE GATT.
- `system.hello` and `system.snapshot` work over real Bluetooth.
- A second connection attempt is rejected or closed cleanly.
- Pairing mode can be started and stopped from the CLI.

## Phase 4: Device Packaging

- [x] Add `mba-bt.service`.
- [x] Add service hardening appropriate for BlueZ GATT/advertising access.
- [ ] Add persistent state directory under `/data/matchbox-audio/bt`.
- [x] Add Yocto install rules for `mba-bt`.
- [ ] Include any required BlueZ config or policy.
- [ ] Add remote smoke-test checks for:
  - [ ] service active
  - [ ] GATT service registration
  - [ ] advertising while pairing mode is active
  - [ ] pairing mode CLI
  - [ ] local fake transport routing
- [ ] Verify A/B update preserves authorized clients.

Exit criteria:

- The target image boots with `mba-bt.service` available.
- Bluetooth control state survives reboot and remote update.
- `mba-player` remains isolated from direct Bluetooth permissions.

## Phase 5: Android Project Skeleton

- [ ] Add Android Gradle project under `android/`.
- [ ] Add Kotlin + Jetpack Compose baseline.
- [ ] Add package/application ID.
- [ ] Add app signing placeholders for debug and local release builds.
- [ ] Document local release APK build/install commands.
- [ ] Add Android Bluetooth permissions.
- [ ] Add `MatchboxTransport` interface.
- [ ] Add fake scripted transport.
- [ ] Add BLE transport implementation shell behind the interface.
- [ ] Add protocol fixture tests on the JVM.
- [ ] Add Compose navigation:
  - [ ] setup
  - [ ] now playing
  - [ ] library
  - [ ] queue
  - [ ] settings
- [ ] Add fake-device mode for UI development.
- [ ] Add view-model tests for initial screens.
- [ ] Add instrumented Compose tests using fake transport.

Exit criteria:

- `./gradlew test` passes.
- `./gradlew connectedDebugAndroidTest` passes on an emulator or attached
  device for fake-transport UI tests.
- A debug APK installs and runs without needing Bluetooth.

## Phase 6: Android BLE Connection

- [ ] Add BLE scan/association flow.
- [ ] Add known-device reconnect flow.
- [ ] Add GATT connect/disconnect.
- [ ] Add Matchbox service discovery.
- [ ] Add characteristic write and notification subscription.
- [ ] Add MTU request and conservative-MTU fallback behavior.
- [ ] Add protocol read/write loop.
- [ ] Add connection status UI.
- [ ] Add permission-denied UI.
- [ ] Add auth-required UI.
- [ ] Add busy-device UI.
- [ ] Add reconnect backoff.
- [ ] Add tests for connection state-machine behavior.
- [ ] Verify app can display `system.snapshot` from the Pi.
- [ ] Verify app reconnects after app process restart.
- [ ] Verify app reconnects after phone lock/unlock.
- [ ] Verify app behavior while phone is connected to car Bluetooth audio.
- [ ] Decide whether RFCOMM fallback still needs a spike.

Exit criteria:

- App connects to the Pi without changing phone Wi-Fi.
- App shows real device status and now-playing snapshot.
- Reconnect behavior is usable on the target phone.

## Phase 7: Core Player Workflows

- [ ] Now-playing screen:
  - [ ] playback state
  - [ ] track title/path
  - [ ] artist/album when available
  - [ ] elapsed/duration
  - [ ] volume
- [ ] Playback controls:
  - [ ] play
  - [ ] pause
  - [ ] toggle
  - [ ] stop
  - [ ] next
  - [ ] previous
  - [ ] seek
  - [ ] volume set
- [ ] Library screen:
  - [ ] root listing
  - [ ] nested directory navigation
  - [ ] paged listing load-more behavior
  - [ ] enqueue file
  - [ ] enqueue directory
  - [ ] up/back behavior
- [ ] Queue screen:
  - [ ] list queue
  - [ ] paged queue load-more behavior if queue exceeds one response
  - [ ] play queue item
  - [ ] play item next
  - [ ] remove item
  - [ ] move item
  - [ ] clear queue
- [ ] Events:
  - [ ] playback changed
  - [ ] track changed
  - [ ] queue changed
  - [ ] device warning/error
- [ ] Rescan:
  - [ ] start rescan
  - [ ] show running/completed state if available

Exit criteria:

- Android covers the same core workflows as the current web app, except search
  and artwork.
- Web app and Android app can both control playback without corrupting state.
- Queue edits remain coherent while playback continues.

## Phase 8: Pairing and Trust Polish

- [ ] Persist authorized client records.
- [ ] Reject untrusted clients outside pairing mode.
- [ ] Show pairing code on PIM483 display.
- [ ] Require app to echo pairing code during trust handshake.
- [ ] Add CLI list/forget flows.
- [ ] Decide and implement hardware-button pairing gesture.
- [ ] Add timeout for pairing mode.
- [ ] Add tests for trust state transitions.

Exit criteria:

- Unknown phones cannot control the player outside pairing mode.
- A new phone can be paired deliberately.
- Authorized clients survive reboot and A/B update.
- Forgotten clients cannot reconnect as trusted clients.

## Phase 9: Field Validation

- [ ] Cold boot in car and connect from Android without touching Wi-Fi.
- [ ] Multiple short trips with power loss between trips.
- [ ] One hour continuous playback and browsing.
- [ ] Large library browse with 30 GB or more of music.
- [ ] Large directory browse that crosses one BLE message page.
- [ ] Phone lock/unlock during playback.
- [ ] Pi reboot while app is open.
- [ ] App process kill/restart.
- [ ] Phone connected to car Bluetooth audio/calls while app controls Matchbox.
- [ ] Verify no audible playback disruption during reconnect.
- [ ] Capture known issues and update this plan.

Exit criteria:

- Android app is reliable enough for normal in-car control.
- Remaining issues are documented and prioritized.
- Wi-Fi hotspot remains a viable fallback path.

## Local Development Loops

Rust protocol loop:

```sh
cargo test -p mba-protocol
```

Rust bridge loop:

```sh
cargo test -p mba-bt
```

Whole Rust workspace:

```sh
cargo test --workspace
```

Android JVM loop:

```sh
cd android
./gradlew test
```

Android UI loop:

```sh
cd android
./gradlew connectedDebugAndroidTest
```

Android APK loop:

```sh
cd android
./gradlew :app:assembleDebug
```

Device smoke loop:

```sh
./update.sh --skip-build --smoke
```

Task names may change as the Android project is created. Keep the local test
surface simple enough that it is practical to run during normal development.

## Deferral List

- RFCOMM fallback implementation.
- Artwork and thumbnail cache.
- Search.
- Multi-client control.
- Play Store distribution.
- Music sync over Bluetooth.
- Bluetooth audio sink mode.
- Full Android hardware-in-loop automation for the initial pairing UI.
