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

or the Phase 3 `mba-bt` BLE-local mode with the deterministic fake player
backend:

```sh
cargo run -p mba-bt -- --ble-local --fake-player \
  --control-socket /tmp/mba-bt/control.sock \
  --state-dir /tmp/mba-bt/state
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

## Real Pi and Android Smoke

The production smoke path uses the packaged `mba-bt.service` on the Pi and an
attached Android phone.

The repeatable helper for the production Android app is:

```sh
tools/android-real-ble-smoke.mjs --timeout 45000
```

It installs `android/app`, wakes the phone, clears stale app sessions, verifies
that the Pi BLE daemon is advertising and idle, reads the Pi player status,
launches the Android app, taps **Connect BLE**, waits until the app UI matches
the Pi status, verifies known-device persistence, force-stops and relaunches
the app to exercise reconnect, and saves a screenshot to
`/tmp/matchbox-android-real-ble-smoke.png`.

The helper uses `JAVA_HOME` when it points at a JDK with `bin/javac`; otherwise
it tries Android Studio's bundled JBR at
`/home/oldman/.progs/android-studio/jbr`. Use `--java-home <path>` to override
that detection.

Use `--no-restart-check` to skip the force-stop/relaunch reconnect pass.
Use `--skip-install` when the current APK is already installed.

1. Deploy the current image and run the device smoke checks:

   ```sh
   ./update.sh --smoke
   ```

2. Confirm the Pi-side Bluetooth daemon is advertising and idle:

   ```sh
   ssh matchbox@matchbox-audio.local 'mba-cli bt status'
   ```

   Expected fields include `advertising: true` and `busy: false`.

3. Install the Android smoke APK:

   ```sh
   cd tools/android-ble-smoke
   JAVA_HOME=/home/oldman/.progs/android-studio/jbr ./gradlew :app:assembleDebug
   adb install -r app/build/outputs/apk/debug/app-debug.apk
   ```

4. Launch the smoke app on the phone, grant Bluetooth permissions, and tap
   **Start BLE Smoke**.

5. The log should show the phone scanning, connecting, requesting MTU 517,
   reading Status, subscribing to TX notifications, sending `system.hello`,
   then sending `system.snapshot`.

6. Compare the `system.snapshot` response in the app log with the real Pi
   status:

   ```sh
   target/debug/mba-cli --server http://matchbox-audio.local:8090 status
   target/debug/mba-cli --server http://matchbox-audio.local:8090 queue
   ```

For the production Android app, install `android/app` instead of
`tools/android-ble-smoke`, tap **Connect BLE**, then compare the now-playing
screen with the same `mba-cli status` output.

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
