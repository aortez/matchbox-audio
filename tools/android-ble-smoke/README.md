# Matchbox Android BLE Smoke App

Minimal Android client for the Phase 0 BLE proof. This is intentionally not the
production app. It exists to prove the phone can scan, connect, subscribe to
notifications, request MTU, write a chunked `system.hello`, and reassemble the
chunked response.

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

Before running the app, run the Pi-side spike:

```sh
cargo run --manifest-path tools/ble-gatt-spike/Cargo.toml
```

For the Matchbox target image, use the cross-build and SSH commands documented
in `../ble-gatt-spike/README.md`.

Expected app flow:

1. Grant Bluetooth permissions.
2. Tap **Start BLE Smoke**.
3. The app scans for service `1cef04f1-966e-43ad-860f-086db4f277d6`.
4. The app connects, requests MTU 517, discovers GATT services, subscribes to
   TX notifications, and writes a chunked `system.hello`.
5. The log should show a complete JSON response from the Pi.

Validation on May 16, 2026:

- Phone: Pixel 7 Pro.
- Pi target: `matchbox-audio.local`.
- Result: scan found `Matchbox Audio`, GATT connected, MTU 517 succeeded,
  Status read succeeded, TX notifications delivered a three-chunk
  `system.hello` response.
- Note: Bluetooth was initially disabled on the phone; enabling Bluetooth fixed
  the first `Bluetooth adapter unavailable or disabled` result.
