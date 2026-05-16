# Matchbox Audio Android App Plan

This plan sequences the Android app and Bluetooth bridge work. The architecture
and rationale live in `docs/android-app-design.md`; this document tracks
execution order, test gates, and deployment milestones.

The bias is local-first development: most protocol, bridge, and app behavior
should be testable without Bluetooth hardware. Real Pi and Android phone tests
are still required before calling the feature usable.

## Current Decisions

- Android uses Bluetooth Classic RFCOMM as the primary transport.
- `mba-bt` is a separate bridge daemon; `mba-player` remains the owner of
  playback, library, queue, and product state.
- `mba-bt` v1 allows one active app connection.
- A second active app connection receives a structured `busy` response.
- BLE is optional and deferred unless Classic discovery/pairing is poor.
- Pairing mode starts as a CLI-triggered flow before hardware-button UX.
- Android v1 does not include artwork.
- Android v1 does not include search.
- The web app remains available for fallback, diagnostics, and admin workflows.
- Private Android deployment starts with local debug installs, then signed APKs.

## Workstreams

### Protocol Contract

Owns the stable client/device language shared by Rust, Android, and eventually
the web app.

- Request, response, event, and error envelopes.
- Frame encoding and parsing.
- Protocol version negotiation.
- Capability reporting.
- JSON fixtures shared by Rust and Android tests.
- Compatibility policy for future protocol changes.

### Device Bridge

Owns Bluetooth exposure and translation into existing Matchbox behavior.

- New `mba-bt` Rust crate.
- Transport abstraction for in-memory, TCP/dev, stdin/stdout/dev, and RFCOMM.
- Fake `mba-player` backend for tests.
- Local protocol exerciser.
- BlueZ RFCOMM profile registration.
- Pairing/trust state under `/data/matchbox-audio/bt`.
- Yocto packaging and `mba-bt.service`.

### Android App

Owns the native user experience.

- Kotlin + Jetpack Compose project under `android/`.
- Compose screens for now playing, library, queue, setup, and settings.
- View models and repository layer.
- Fake and scripted transports for local development.
- Real RFCOMM transport.
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
- [ ] Validate `bluer` RFCOMM/Profile API support on the development host.
- [ ] Validate `bluer` RFCOMM/Profile API support on the target image.
- [ ] Decide whether direct `zbus` BlueZ integration is needed.
- [ ] Confirm Android target phone can discover and pair with a custom RFCOMM
  service without BLE advertising.
- [ ] Decide first private APK distribution path after local installs.

Exit criteria:

- The RFCOMM server approach is proven on the Pi or a clear fallback is chosen.
- The target Android phone can complete at least one manual RFCOMM connection.
- The project has enough confidence to add `mba-bt` to the workspace.

## Phase 1: Shared Protocol Fixtures

- [ ] Add shared protocol fixture directory.
- [ ] Define request, response, event, and error envelopes in `mba-protocol`.
- [ ] Define frame format and max frame size.
- [ ] Add Rust encode/decode tests for core fixtures.
- [ ] Add Rust frame parser tests:
  - [ ] partial read
  - [ ] oversized frame
  - [ ] malformed JSON
  - [ ] unknown method
  - [ ] unsupported protocol version
- [ ] Define initial method names:
  - [ ] `system.hello`
  - [ ] `system.snapshot`
  - [ ] `events.subscribe`
  - [ ] playback methods
  - [ ] library list methods
  - [ ] queue methods
- [ ] Define stable error codes:
  - [ ] `bad_request`
  - [ ] `unauthorized`
  - [ ] `auth_required`
  - [ ] `busy`
  - [ ] `not_found`
  - [ ] `unsupported_method`
  - [ ] `unsupported_version`
  - [ ] `player_unavailable`
  - [ ] `internal`

Exit criteria:

- `cargo test -p mba-protocol` validates the protocol fixtures.
- The envelope supports request/response and unsolicited events.
- Fixture format is stable enough for Android tests to consume.

## Phase 2: `mba-bt` Local Bridge

- [ ] Add `crates/mba-bt`.
- [ ] Add `mba-bt` to the workspace.
- [ ] Implement transport trait or equivalent stream abstraction.
- [ ] Implement in-memory transport tests.
- [ ] Implement framed protocol read/write.
- [ ] Implement `system.hello`.
- [ ] Implement `system.snapshot`.
- [ ] Implement event subscription plumbing with fake events.
- [ ] Implement one-active-client gate.
- [ ] Implement structured `busy` response for a second client.
- [ ] Add fake `mba-player` backend.
- [ ] Add local protocol exerciser command.
- [ ] Add tests for:
  - [ ] connect/disconnect cleanup
  - [ ] auth-required response
  - [ ] busy response
  - [ ] player unavailable response
  - [ ] malformed frame handling
  - [ ] unsupported method response
  - [ ] event delivery to subscribed client

Exit criteria:

- `cargo test -p mba-bt` passes without Bluetooth hardware.
- A local developer tool can send framed requests and receive framed responses.
- The bridge can be tested against a fake player backend.

## Phase 3: BlueZ RFCOMM Integration

- [ ] Add BlueZ RFCOMM implementation behind the transport boundary.
- [ ] Register Matchbox RFCOMM profile with a stable service UUID.
- [ ] Advertise a useful device/service name.
- [ ] Accept a manual RFCOMM connection from a Linux client.
- [ ] Accept a manual RFCOMM connection from the Android target phone.
- [ ] Verify automatic channel discovery from Android.
- [ ] Add service logging for connection, disconnect, and protocol errors.
- [ ] Add `mba-cli bt status`.
- [ ] Add `mba-cli bt pairing start --timeout <seconds>`.
- [ ] Add `mba-cli bt pairing stop`.
- [ ] Add `mba-cli bt clients`.
- [ ] Add `mba-cli bt forget <client>`.

Exit criteria:

- A real Android phone can connect to the Pi over RFCOMM.
- `system.hello` and `system.snapshot` work over real Bluetooth.
- A second connection attempt is rejected or closed cleanly.
- Pairing mode can be started and stopped from the CLI.

## Phase 4: Device Packaging

- [ ] Add `mba-bt.service`.
- [ ] Add service hardening appropriate for BlueZ/RFCOMM access.
- [ ] Add persistent state directory under `/data/matchbox-audio/bt`.
- [ ] Add Yocto install rules for `mba-bt`.
- [ ] Include any required BlueZ config or policy.
- [ ] Add remote smoke-test checks for:
  - [ ] service active
  - [ ] profile registration
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
- [ ] Add Android Bluetooth permissions.
- [ ] Add `MatchboxTransport` interface.
- [ ] Add fake scripted transport.
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

## Phase 6: Android RFCOMM Connection

- [ ] Add Classic Bluetooth scan/association flow.
- [ ] Add known-device reconnect flow.
- [ ] Add RFCOMM socket connect/disconnect.
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
  - [ ] enqueue file
  - [ ] enqueue directory
  - [ ] up/back behavior
- [ ] Queue screen:
  - [ ] list queue
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

- BLE discovery/pairing assist.
- Artwork and thumbnail cache.
- Search.
- Multi-client control.
- Play Store distribution.
- Music sync over Bluetooth.
- Bluetooth audio sink mode.
- Full Android hardware-in-loop automation for the initial pairing UI.
