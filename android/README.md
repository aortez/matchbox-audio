# Matchbox Audio Android App

Native Android client for Matchbox Audio.

Current status:

- Kotlin + Jetpack Compose app skeleton.
- Fake `MatchboxTransport` for local UI development.
- BLE `MatchboxTransport` implementation shell for scan, GATT connect, service
  discovery, MTU request, TX notification subscription, chunked writes, and
  chunked response reassembly.
- Now-playing screen backed by fake `system.snapshot` data.
- JVM tests for protocol fixture parsing, BLE chunk framing, and view-model
  state.
- Compose instrumented tests using fake data.

Local commands:

```sh
JAVA_HOME=/home/oldman/.progs/android-studio/jbr ./gradlew test
JAVA_HOME=/home/oldman/.progs/android-studio/jbr ./gradlew connectedDebugAndroidTest
JAVA_HOME=/home/oldman/.progs/android-studio/jbr ./gradlew installDebug
```

If connected Compose tests report that no hierarchy was found, wake the attached
phone first:

```sh
adb shell input keyevent KEYCODE_WAKEUP
adb shell wm dismiss-keyguard
```

Capture the app from an attached phone:

```sh
../tools/android-capture.sh --output /tmp/matchbox-android-now-playing.png
```

Run the real Pi plus Android BLE smoke helper:

```sh
../tools/android-real-ble-smoke.mjs --timeout 45000
```

The helper installs the debug APK, connects the app over BLE, compares the
visible now-playing fields with `http://matchbox-audio.local:8090/api/v1/status`,
and captures `/tmp/matchbox-android-real-ble-smoke.png`. It uses `JAVA_HOME`
when that points at a JDK with `bin/javac`; otherwise it tries Android Studio's
bundled JBR at `/home/oldman/.progs/android-studio/jbr`.

The checked-in Gradle project expects a local Android SDK. Keep
`local.properties` out of git; Android Studio can generate it, or copy the SDK
path used by another local Android project.

The checked-in UI still uses `FakeMatchboxTransport`. The BLE transport exists
behind `MatchboxTransport`, and the app has a Connect BLE action that requests
runtime Bluetooth permissions before switching the now-playing view to the BLE
transport. Known-device reconnect behavior is still pending.
