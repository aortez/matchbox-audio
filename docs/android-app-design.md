# Matchbox Audio Android App Design

## Purpose

The Android app is the planned full-featured in-car control surface for
Matchbox Audio when the user does not want to switch the phone onto the
Matchbox Wi-Fi hotspot or run a phone hotspot.

Execution sequencing lives in `docs/android-app-plan.md`.

The app should feel like the primary player UI, not a simple Bluetooth remote.
It should support browsing, search, queue editing, now-playing context,
playback controls, settings, and device status over a Bluetooth link.

The existing web app remains useful as a fallback, development surface, and
admin interface. The Android app supplements it; it does not require removing
or replacing the web app.

## Goals

- Avoid phone Wi-Fi switching for normal in-car use.
- Provide a full player UI over Bluetooth.
- Reuse the existing Matchbox Audio product model and API semantics.
- Keep Bluetooth-specific permissions and service hardening outside
  `mba-player`.
- Share protocol contracts between Rust, Android, and the web app.
- Support reliable reconnect after car power cycles, phone lock/unlock, and app
  restarts.
- Keep the Wi-Fi web UI available for fallback, music sync, updates, and
  diagnostics.

## Non-Goals

- Bluetooth audio sink support.
- Music sync or file upload over Bluetooth in the first Android app version.
- Replacing MPD as the playback engine.
- Replacing the web app as the only supported control surface.
- Artwork transfer, bulk library sync, or large diagnostics over Bluetooth in
  the first Android app version.
- Depending on Play Store deployment for development or personal-device use.

## Transport Decision

Use BLE GATT as the main Android app control transport.

BLE is more constrained than Bluetooth Classic RFCOMM, but it matches the
near-term product scope if the protocol is designed carefully. Android v1 needs
metadata, playback commands, queue edits, device state, event notifications, and
paged library browsing. It does not need artwork, bulk library sync, music
transfer, or large diagnostics over Bluetooth.

The cost is that `mba-bt` must provide a real message transport over GATT:
chunking, reassembly, request/response correlation, notification flow control,
bounded payloads, reconnect cleanup, and paged result shapes for anything that
can grow. That complexity should live below the app-level Matchbox protocol so
the same request, response, event, and error envelopes can still be exercised
over in-memory transports and local development tools.

Recommended first implementation:

```text
Android app
  -> BLE GATT
  -> mba-bt bridge daemon
  -> localhost mba-player API
  -> MPD
```

Fallback if BLE proves unreliable on the target phone:

```text
Android app
  -> Bluetooth Classic RFCOMM
  -> mba-bt bridge daemon
  -> localhost mba-player API
  -> MPD
```

## BLE Scope

BLE carries the main control protocol, but only within bounded message sizes.

Useful BLE responsibilities:

- Advertise that the device is in Matchbox pairing mode.
- Expose device name, firmware version, and stable device identifier.
- Provide a friendly "identify" action while pairing.
- Carry pairing/trust handshake messages.
- Carry playback commands, now-playing snapshots, queue edits, and events.
- Carry paged library and queue listings.
- Reuse or revive the existing Improv Wi-Fi provisioner for home Wi-Fi setup.

Responsibilities BLE should not take in v1:

- Artwork transfer.
- Bulk full-library synchronization.
- Music upload or sync.
- Large diagnostics export.
- System update transfer.

Design constraints:

- Treat BLE as message-oriented, not as a raw byte stream.
- Request a larger MTU when Android allows it, but keep the protocol correct at
  conservative MTU sizes.
- Define a maximum app-level message size and reject larger responses with a
  structured error.
- Page any listing that can grow beyond one small response.
- Send events as best-effort notifications and recover state with
  `system.snapshot` after reconnect.
- Allow only one active control session initially.
- Keep RFCOMM as a fallback transport behind the same `MatchboxTransport`
  boundary if field testing exposes BLE problems.

## Device Architecture

Add a new Rust daemon:

- Crate: `mba-bt`
- Service: `mba-bt.service`
- Runtime user: dedicated unprivileged user if BlueZ permissions allow it;
  otherwise a narrowly hardened service with only the Bluetooth permissions it
  needs.
- Responsibilities:
  - Register a Matchbox BLE GATT service and advertisement with BlueZ.
  - Accept one active control session initially.
  - Chunk, reassemble, parse, and validate protocol messages.
  - Proxy supported commands to `mba-player` over localhost HTTP.
  - Subscribe or poll for player state and send events to the app.
  - Enforce pairing authorization and client trust.
  - Persist authorized client records under `/data/matchbox-audio/bt`.

`mba-player` remains the owner of playback, library, queue, and product state.
Bluetooth should translate into the same command model used by the web app and
CLI.

The Linux implementation should be validated with BlueZ's D-Bus GATT and
advertising APIs. The existing pi-base Improv Wi-Fi provisioner already proves
the basic `bluer` GATT path on Raspberry Pi-class devices. If Rust crate support
is too thin for the Matchbox service shape, isolate direct `zbus` integration
behind a small module so the player protocol and business logic stay testable
without Bluetooth hardware.

## Android Architecture

Use Kotlin and Jetpack Compose for the Android app.

Initial modules:

```text
android/
  settings.gradle.kts
  build.gradle.kts
  app/
    src/main/
      AndroidManifest.xml
      kotlin/.../MainActivity.kt
      kotlin/.../bluetooth/
      kotlin/.../protocol/
      kotlin/.../player/
      kotlin/.../ui/
```

App layers:

- UI: Compose screens for now playing, library, queue, search, and settings.
- View models: app state, loading/error state, selected device, queue edits,
  current browse path.
- Repository: player operations expressed in Matchbox domain terms.
- Transport: BLE GATT implementation plus a fake transport for tests.
- Protocol: generated or manually synchronized DTOs and message envelopes.
- Cache: local library/artwork cache, likely Room or simple file cache after
  the first end-to-end version.

Android permissions:

- Use Android 12+ nearby-device Bluetooth permissions.
- Use BLE scan/connect permissions and the Companion Device Manager flow if it
  improves device association without asking for location permissions.
- Do not require location unless Android version or discovery behavior makes it
  unavoidable.

## Shared Code Strategy

The first unification point should be the protocol contract, not shared UI.

Recommended repo direction:

```text
crates/mba-protocol/        Rust domain DTOs and protocol tests
protocol/                   JSON schema or generated contract artifacts
android/app/.../protocol/   Kotlin generated or synchronized DTOs
crates/mba-player/web/      Current web app, later TypeScript DTOs if migrated
```

The goal is to keep these clients speaking the same product language:

- Rust daemon and CLI.
- Android app over BLE GATT.
- Web app over HTTP/WebSocket.

Preferred near-term approach:

- Define explicit command, response, event, and error envelopes in
  `mba-protocol`.
- Add serialization tests with stable JSON fixtures.
- Generate or validate equivalent Kotlin DTOs from the same schema once the
  envelope stabilizes.
- Keep the existing web app as-is until the protocol shape is proven.

Possible later unification:

- Native Android app plus web app share generated DTOs only.
- Kotlin Multiplatform shares protocol, state reducers, and some view-model
  logic.
- React/TypeScript plus Capacitor shares most UI between web and Android, with
  a native Bluetooth plugin.

Current recommendation: build native Android first and share the protocol. Do
not choose a cross-platform UI framework until the Bluetooth product flow is
working on the real phone.

## Protocol

Use transport-neutral JSON messages for the app protocol. Local development
streams can use a fixed-width length prefix. BLE carries the same logical
messages through a small chunk envelope over GATT characteristics.

Logical message:

```text
uint32_le json_length
utf8_json_payload
```

BLE characteristics:

```text
Matchbox Control Service
  capabilities/status: read
  rx: write or write-with-response, app -> device chunks
  tx: notify, device -> app chunks
```

Initial UUID layout:

| Item | UUID | Properties | Purpose |
| ---- | ---- | ---------- | ------- |
| Matchbox Control Service | `1cef04f1-966e-43ad-860f-086db4f277d6` | primary service | Groups the Matchbox Android control protocol. |
| Status characteristic | `bd539314-4637-416b-a3b5-804fecd5b792` | read | Reports protocol versions, transport limits, device identity, pairing state, and busy state. |
| RX characteristic | `fbf39e22-bb07-49bf-bfa0-3dbdfc47769b` | write-with-response | Receives app-to-device protocol chunks. |
| TX characteristic | `fcc9055c-34e3-46d9-a010-bd8a4f180b0c` | notify | Sends device-to-app protocol chunks after the app subscribes. |

Start with write-with-response for RX. It is conservative, but it gives Android
an explicit success/failure callback for each chunk. If later profiling shows
that request throughput is too slow, add write-without-response behind the same
transport abstraction after flow control is proven.

Initial advertisement:

- Advertise the Matchbox Control Service UUID.
- Use a local name derived from the device name, such as `Matchbox Audio` or the
  configured hostname.
- Include only small service data if needed for pairing-mode state or protocol
  version. Keep detailed state in the readable Status characteristic because BLE
  advertisements have tight size limits.

BLE chunk envelope:

```text
0..1    magic bytes: "MB"
2       chunk protocol version: 1
3       flags
4..7    transport message_id: u32_le
8..9    chunk_index: u16_le
10..11  chunk_count: u16_le
12..15  total_message_len: u32_le
16..    payload_fragment
```

Initial flags:

| Bit | Name | Meaning |
| --- | ---- | ------- |
| 0 | first_chunk | This is the first chunk for `message_id`. |
| 1 | last_chunk | This is the final chunk for `message_id`. |

Initial transport rules:

- Use one in-flight message per direction.
- Chunks for a message must arrive in order.
- `chunk_index` is zero-based.
- `chunk_count` must be known up front and must match the final chunk.
- `total_message_len` is the full JSON message length before chunking.
- Reject bad magic bytes, unsupported chunk versions, duplicate chunks, missing
  chunks, out-of-order chunks, inconsistent lengths, and oversized messages.
- Clean up partial messages on disconnect.
- Keep chunk boundaries independent from JSON message boundaries.

Initial limits:

| Limit | Value | Meaning |
| ----- | ----- | ------- |
| `max_message_bytes` | 16 KiB | Hard limit for any complete JSON request, response, or event. |
| `target_response_bytes` | 8 KiB | Soft target for normal responses, especially paged listings. |
| `target_gatt_value_bytes` | 244 bytes | Conservative starting size for one characteristic write or notification. |
| `chunk_header_bytes` | 16 bytes | Header size before each payload fragment. |
| `target_chunk_payload_bytes` | 228 bytes | Payload per chunk when using the conservative 244-byte GATT value target. |

Android should request a larger MTU when connected, but the protocol must remain
correct at conservative GATT value sizes. If the negotiated link supports larger
values reliably, the transport can increase chunk payload size without changing
the app-level protocol.

Paging defaults:

| Setting | Value | Meaning |
| ------- | ----- | ------- |
| `default_page_limit` | 50 items | The normal Android request size for library and queue pages. |
| `max_page_limit` | 100 items | The largest page size the device accepts. |
| `target_page_response_bytes` | 8 KiB | Device should try to keep a page response under this size. |

The byte target wins over item count. If 50 items would exceed the response
target, the device may return fewer items with `has_more: true`. If a single
logical response would exceed `max_message_bytes`, the device rejects it with a
structured error and the app should retry with a smaller page or narrower
request.

Paged listing requests should include `cursor` and `limit` parameters. Responses
should include `next_cursor` and `has_more` so Android can load the next page on
demand instead of asking BLE to carry a whole library or queue.

Request:

```json
{
  "type": "request",
  "id": 17,
  "method": "library.list",
  "params": {
    "path": "Air/Moon Safari"
  }
}
```

Response:

```json
{
  "type": "response",
  "id": 17,
  "ok": true,
  "result": {
    "path": "Air/Moon Safari",
    "directories": [],
    "tracks": []
  }
}
```

Event:

```json
{
  "type": "event",
  "seq": 42,
  "event": "playback.changed",
  "payload": {
    "state": "play",
    "volume": 60
  }
}
```

Error:

```json
{
  "type": "response",
  "id": 17,
  "ok": false,
  "error": {
    "code": "not_found",
    "message": "library path not found"
  }
}
```

Connection handshake:

1. App scans for the Matchbox BLE advertisement.
2. App connects and discovers the Matchbox Control Service.
3. App subscribes to the `tx` notification characteristic.
4. App sends `system.hello` with app version and supported protocol versions.
5. Device responds with selected protocol version, capabilities, device name,
   build version, BLE transport limits, and pairing/trust state.
6. If untrusted, device requires pairing authorization before accepting control
   commands.
7. App requests an initial snapshot.
8. App subscribes to events.

Core method groups:

- `system.hello`
- `system.snapshot`
- `playback.play`
- `playback.pause`
- `playback.toggle`
- `playback.stop`
- `playback.next`
- `playback.previous`
- `playback.seek`
- `playback.volume.set`
- `library.list`
- `library.search`
- `library.rescan`
- `queue.list`
- `queue.addFile`
- `queue.addDirectory`
- `queue.play`
- `queue.playNext`
- `queue.remove`
- `queue.move`
- `queue.clear`
- `events.subscribe`
- `events.unsubscribe`
- `device.identify`
- `device.settings.get`

Core events:

- `playback.changed`
- `track.changed`
- `queue.changed`
- `library.scan.started`
- `library.scan.progress`
- `library.scan.finished`
- `network.changed`
- `device.warning`
- `device.error`

## Security and Pairing

Bluetooth pairing alone should not be treated as full authorization.

Recommended first trust model:

- Device accepts unknown clients only while pairing mode is active.
- Pairing mode is started by a long hardware-button action or a local CLI
  command.
- During pairing, the PIM483 display shows a short code.
- The Android app must echo that code during the trust handshake.
- The device stores an authorized client record under
  `/data/matchbox-audio/bt`.
- Normal control commands are rejected for untrusted clients.
- A CLI command can list and forget authorized clients.

This keeps the normal in-car UX simple while avoiding an always-open Bluetooth
control surface.

## Artwork and Caching

Bluetooth is acceptable for metadata and normal control messages. It should be
treated carefully for artwork and large library payloads.

Initial behavior:

- Browse live directory listings.
- Cache recently visited directories in the app.
- Show metadata from the listing and now-playing responses.
- Use placeholder artwork or cached thumbnails until the artwork cache exists.

Later behavior:

- Add small thumbnail endpoints to the device protocol.
- Cache thumbnails in the Android app.
- Let the app request artwork by stable artwork ID and size.
- Avoid bulk full-library artwork downloads.

Music sync, system updates, and large diagnostics remain Wi-Fi/SSH workflows.

## Development Plan

### Phase A: Protocol Contract

- Define the Bluetooth message envelope in `mba-protocol`.
- Add JSON fixture tests for requests, responses, events, and errors.
- Decide on generated Kotlin DTOs versus manually maintained Kotlin DTOs.
- Define protocol versioning and capability negotiation.
- Define BLE transport limits, chunking, reassembly, and max message size.
- Define paged response shapes for library and queue listings.
- Add a fake transport harness for protocol round-trip tests.
- Add a shared fixture directory that both Rust and Android tests consume.
- Define a transport abstraction before any real Bluetooth code lands.

Exit criteria:

- Rust can encode/decode the protocol fixtures.
- Kotlin can encode/decode the same fixtures, even if the Android UI is not
  started yet.
- BLE chunking can round-trip fixture messages and reject malformed chunks.
- The protocol can be exercised through an in-memory stream without BlueZ,
  Android, or Bluetooth hardware.

### Phase B: `mba-bt` Bridge Spike

- Add the `mba-bt` crate.
- Implement the protocol against an in-memory test transport.
- Implement the BLE-style chunker/reassembler against in-memory tests.
- Proxy `system.hello`, `system.snapshot`, and a small playback command subset
  to local `mba-player`.
- Add a fake `mba-player` backend for deterministic bridge tests.
- Add local CLI test tools that can speak the framed protocol over stdin/stdout
  or TCP before real BLE is working.
- Validate BlueZ GATT service registration and advertising on the Pi.
- Package `mba-bt.service` in the Yocto image.

Exit criteria:

- `cargo test` covers framing, bridge routing, disconnect cleanup, auth gates,
  and one-active-client behavior without Bluetooth hardware.
- A physical Android phone can scan for the BLE advertisement, connect, subscribe
  to notifications, and exchange `system.hello`.
- `system.snapshot` returns real player status.
- The service survives disconnect/reconnect without restarting the whole player.

### Phase C: Android App Skeleton

- Add the Gradle Android project under `android/`.
- Create Compose navigation for now playing, library, queue, search, and
  settings.
- Build the app against a fake player repository first.
- Add a fake transport build flavor or dependency-injection path for local UI
  development and tests.
- Add JVM tests for protocol fixtures, view models, and connection state.
- Add instrumented Compose tests that run against the fake transport.
- Add BLE scan/connect, GATT discovery, characteristic write/notify, and
  protocol read/write.
- Add connection status, error display, and manual reconnect.

Exit criteria:

- Android tests pass without a phone or Bluetooth adapter for non-radio logic.
- Debug APK installs locally.
- App connects to the Pi and displays the current snapshot.
- App can reconnect after force-closing and reopening.

### Phase D: Full Player Features

- Implement now playing and transport controls.
- Implement library browsing.
- Implement queue listing, play item, add file/directory, remove, move, and
  clear.
- Implement search after the device-side search API lands.
- Add scan/rescan controls and progress display.
- Add event subscriptions so the UI does not depend on polling.

Exit criteria:

- The Android app covers the same core workflows as the web app.
- Normal use does not require changing phone Wi-Fi state.
- Web app and Android app can both control the device without corrupting queue
  state.

### Phase E: Pairing, Trust, and Field Polish

- Add hardware-button pairing mode.
- Show pairing code on the PIM483 display.
- Store and revoke authorized clients.
- Add automatic reconnect policy.
- Add app-side cache for recent library paths and thumbnails.
- Add settings for device name, reconnect behavior, and diagnostics export.

Exit criteria:

- A new phone can be paired deliberately.
- An unknown phone cannot control the player outside pairing mode.
- The app reconnects after Pi power cycle and phone lock/unlock.

## Deployment Plan

Device deployment:

- Add `mba-bt` to the Rust workspace.
- Add `mba-bt.service` and any BlueZ policy/config to the Yocto recipe.
- Preserve authorized client data under `/data/matchbox-audio/bt`.
- Include `mba-cli` commands for Bluetooth status and authorized-client
  management.
- Reuse the existing A/B remote update path.

Android deployment:

- Start with local debug APK installs from Gradle.
- Add a signed release APK once the app controls real playback reliably.
- Keep the release signing key outside git; only checked-in Gradle placeholders
  should refer to local/private signing configuration.
- Distribute the signed APK manually at first.
- Use GitHub Releases, Firebase App Distribution, or Play internal testing only
  after there are multiple testers.
- Version the app and device protocol independently; the handshake selects the
  compatible protocol version.

Compatibility policy:

- App should show a clear error when the device protocol is too old.
- Device should reject unsupported app protocol versions with a structured
  error.
- Backward-compatible additions should be advertised through capabilities.

## Testing Plan

Testing should be local-first. The project may later add CI, but the normal
development loop should already support deterministic tests without requiring
Bluetooth hardware for every change.

Recommended local commands:

- `cargo test --workspace`
- `cargo test -p mba-bt`
- `./gradlew test`
- `./gradlew connectedDebugAndroidTest` when an emulator or phone is attached
- `./gradlew :app:assembleDebug`

Exact Gradle task names can change once the Android project exists; the
principle is that Rust, Android JVM, and Android instrumented tests should each
have a single obvious local command.

### Testability Hooks

Build these seams into the first implementation:

- A Rust transport abstraction for `mba-bt`, so the protocol bridge can run over
  in-memory transports, TCP loopback, stdin/stdout tools, or real BLE GATT.
- A fake `mba-player` client for bridge tests.
- A Kotlin `MatchboxTransport` interface with fake, scripted, and real BLE GATT
  implementations.
- A fake Android player repository for Compose UI and view-model tests.
- A shared fixture directory for JSON protocol examples.
- A developer-only fake-device mode in the Android app.
- A local protocol exerciser that can send framed requests and print framed
  responses without Android.

These seams are product code quality features, not test-only shortcuts. They
also make debugging the real Bluetooth connection easier because the protocol
can be verified independently from the radio.

Rust tests:

- Protocol fixture serialization/deserialization.
- Chunk/reassembly tests for partial messages, oversized messages, malformed
  chunks, disconnect cleanup, and missing chunks.
- Frame parser tests for local stream transports, oversized frames, malformed
  JSON, and disconnects.
- `mba-bt` bridge tests against a fake `mba-player`.
- Command authorization tests.
- Reconnect and one-active-client behavior.
- Fake transport tests for hello, snapshot, request routing, event subscribe,
  auth-required, busy, unsupported method, player unavailable, and malformed
  chunk/message handling.

Android unit tests:

- Protocol fixture tests using the same JSON examples as Rust.
- View-model tests with fake repository data.
- Connection state-machine tests.
- Cache behavior tests.
- Error rendering tests for unavailable device, auth required, and protocol
  mismatch.
- Scripted fake-transport tests for reconnect backoff, stale response IDs,
  out-of-order events, and transport disconnects.

Android instrumented tests with fake transport:

- Compose navigation and layout smoke tests.
- Now-playing screen updates from scripted events.
- Library browse screen with nested paths.
- Queue edit flows with fake success and fake errors.
- Setup screens for disconnected, permission denied, auth required, and busy.
- Configuration changes and process recreation for the main screens.

Android instrumented/manual tests:

- Pairing flow on the target phone.
- BLE scan/connect/disconnect.
- GATT service discovery, write, notification subscription, and MTU negotiation.
- Reconnect after phone lock/unlock.
- Reconnect after app process kill.
- Reconnect after Pi power cycle.
- Behavior while phone is also connected to car Bluetooth audio/calls.
- Behavior with phone Wi-Fi left on its normal network and with Wi-Fi disabled.
- Long library browse session.
- Queue editing while playback continues.

Emulation:

- Use Android emulator or Gradle managed devices for UI tests that do not need
  real Bluetooth.
- Treat Android emulator Bluetooth support as an optional spike for the real BLE
  path, not as a required foundation for the project.
- Consider BlueZ virtual-controller tools only after the real BLE bridge is
  working on the Pi; they are useful for service-level experiments but should not
  block app development.

Device integration tests:

- `mba-bt.service` starts, registers the GATT service, and advertises in pairing
  mode.
- `mba-player` remains isolated from Bluetooth permissions.
- Authorized clients persist across reboot and A/B update.
- Forgetting a client prevents reconnect/control.
- Bluetooth control and web control can operate at the same time.
- Bluetooth control fails gracefully when MPD is unavailable.
- SSH-driven smoke test starts pairing mode, checks service status, checks
  GATT registration, advertising, and verifies local protocol routing through a
  non-Bluetooth test transport.

Field tests:

- Cold boot in car, app reconnects without touching Wi-Fi.
- Multiple short trips with power loss between trips.
- At least one hour of continuous playback and browsing.
- Large-library browsing with 30 GB or more of music.
- Large directory browsing that crosses one BLE message page.
- No audible playback disruption during Bluetooth reconnect.

Hardware-in-loop tests:

- Keep these separate from the normal local unit-test loop.
- Use a real Pi and real Android phone over USB/ADB when practical.
- Automate post-pairing checks first: install debug APK, launch app, connect to
  the known device, send commands, verify state through `mba-cli`, reboot the
  Pi, and verify reconnect.
- Allow the first pairing/bonding step to remain manual until the product flow
  is stable enough to justify automating device-specific Android UI.

## Resolved Decisions

- BLE GATT is the primary Android app control transport.
- RFCOMM is a fallback transport only if BLE field testing is not good enough.
- The first `mba-bt` release allows only one active app connection.
- A second active client receives a structured `busy` response.
- Android v1 shows no artwork because the device-side artwork cache is not
  implemented yet.
- Android v1 does not implement search because the web app and device-side
  search/indexing are not ready yet.
- Initial pairing mode should be CLI-triggered first because that is easiest to
  test. Hardware-button pairing can follow after the service flow works.
- Android deployment starts with local debug APK installs, then manually
  distributed signed release APKs. Firebase App Distribution, GitHub Releases,
  or Play internal testing are later options if more testers are needed.

## Remaining Questions

- Verify reconnect behavior while the phone is connected to car Bluetooth audio
  or calls.
- If later implementation exposes a `bluer` limitation, decide whether to use
  direct `zbus` integration for BlueZ GATT and advertising APIs.
- What exact hardware-button gesture should enter Bluetooth pairing mode after
  the CLI path is proven?

## Initial Recommendation

Build in this order:

1. Shared protocol fixtures.
2. `mba-bt` bridge with fake and BLE-chunked transports.
3. BlueZ BLE GATT advertising/service spike on the Pi.
4. Native Kotlin/Compose app with fake data.
5. Android BLE connection and snapshot display.
6. Full library and queue workflows.
7. Pairing/trust polish.
8. RFCOMM fallback only if BLE field testing is not good enough.

This keeps the project moving toward a full Android player while avoiding early
commitment to cross-platform UI, Play Store packaging, or large Bluetooth data
features.

## References

- Android BLE overview:
  <https://developer.android.com/develop/connectivity/bluetooth/ble/ble-overview>
- Android Bluetooth data transfer:
  <https://developer.android.com/develop/connectivity/bluetooth/transfer-data>
- Android Bluetooth permissions:
  <https://developer.android.com/develop/connectivity/bluetooth/bt-permissions>
- Android companion device pairing:
  <https://developer.android.com/develop/connectivity/bluetooth/companion-device-pairing>
- Android `BluetoothGatt` API:
  <https://developer.android.com/reference/android/bluetooth/BluetoothGatt>
- Android `BluetoothSocket` API for possible RFCOMM fallback:
  <https://developer.android.com/reference/android/bluetooth/BluetoothSocket>
- Android test types and local/instrumented tests:
  <https://developer.android.com/training/testing/fundamentals>
- Gradle managed devices:
  <https://developer.android.com/studio/test/gradle-managed-devices>
- Android emulator networking and Bluetooth notes:
  <https://developer.android.com/studio/run/emulator-networking>
