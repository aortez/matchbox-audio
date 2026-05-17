# Matchbox Android BLE Smoke App

Minimal Android client for the Phase 0 BLE proof. This is intentionally not the
production app. It exists to prove the phone can scan, connect, subscribe to
notifications, request MTU, write chunked `system.hello` and `system.snapshot`
requests, and reassemble chunked responses.

Open this directory in Android Studio, or build from the command line with
Gradle, the Android Gradle plugin, and an Android SDK installed:

```sh
./gradlew :app:assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

If the shell build fails because Gradle cannot find a Java compiler, point
`JAVA_HOME` at Android Studio's bundled JDK first:

```sh
JAVA_HOME=/home/oldman/.progs/android-studio/jbr ./gradlew :app:assembleDebug
```

Before running the app, run either the original Pi-side spike:

```sh
cargo run --manifest-path tools/ble-gatt-spike/Cargo.toml
```

or the Phase 3 `mba-bt` BLE-local mode:

```sh
cargo run -p mba-bt -- --ble-local --control-socket /tmp/mba-bt/control.sock
```

While `mba-bt --ble-local` is running, local daemon state can be inspected with:

```sh
cargo run -p mba-cli -- bt --socket /tmp/mba-bt/control.sock status
```

The pairing window can be opened and closed over the same local admin socket:

```sh
cargo run -p mba-cli -- bt --socket /tmp/mba-bt/control.sock pairing start --timeout 120
cargo run -p mba-cli -- bt --socket /tmp/mba-bt/control.sock pairing stop
```

For the Matchbox target image, use the cross-build and SSH commands documented
in `../ble-gatt-spike/README.md`.

Expected app flow:

1. Grant Bluetooth permissions.
2. Tap **Start BLE Smoke**.
3. The app scans for service `1cef04f1-966e-43ad-860f-086db4f277d6`.
4. The app connects, requests MTU 517, discovers GATT services, subscribes to
   TX notifications, and writes a chunked `system.hello`.
5. After the hello response completes, the app writes a chunked
   `system.snapshot`.
6. The log should show complete JSON responses from the Pi.

Validation on May 16, 2026 PDT:

- Phone: Pixel 7 Pro.
- Pi target: `matchbox-audio.local`.
- Result with `mba-bt --ble-local`: scan found `Matchbox Audio`, GATT
  connected, MTU 517 succeeded, Status read succeeded, and TX notifications
  delivered three-chunk `system.hello` and `system.snapshot` responses.
- Note: Android GATT writes are one-at-a-time. The smoke app waits for
  `onCharacteristicWrite` before sending `system.snapshot`.
- Note: `mba-bt` now treats TX notification subscription as the active app
  session, so the Status payload can report `busy` and active-client details.
- Note: force-stopping the app closed TX notifications and `mba-bt` cleared the
  active BLE session.
- Note: Bluetooth was initially disabled on the phone; enabling Bluetooth fixed
  the first `Bluetooth adapter unavailable or disabled` result.
