# Matchbox Audio Android App

Native Android client for Matchbox Audio.

Current status:

- Kotlin + Jetpack Compose app skeleton.
- Fake `MatchboxTransport` for local UI development.
- Now-playing screen backed by fake `system.snapshot` data.
- JVM tests for protocol fixture parsing and view-model state.
- Compose instrumented tests using fake data.

Local commands:

```sh
JAVA_HOME=/home/oldman/.progs/android-studio/jbr ./gradlew test
JAVA_HOME=/home/oldman/.progs/android-studio/jbr ./gradlew connectedDebugAndroidTest
JAVA_HOME=/home/oldman/.progs/android-studio/jbr ./gradlew installDebug
```

Capture the app from an attached phone:

```sh
../tools/android-capture.sh --output /tmp/matchbox-android-now-playing.png
```

The checked-in Gradle project expects a local Android SDK. Keep
`local.properties` out of git; Android Studio can generate it, or copy the SDK
path used by another local Android project.

BLE integration is intentionally not part of this first skeleton. The next app
slice should add a BLE transport shell behind `MatchboxTransport`, then a
scripted host-to-phone BLE smoke test can run against `mba-bt --ble-local`.
