# Matchbox BLE GATT Spike

Small development-host spike for validating the BLE shape planned for the
Android app. This is intentionally outside the main Rust workspace until the
BlueZ and Android behavior are proven.

It exposes one custom GATT service:

- Matchbox Control Service: `1cef04f1-966e-43ad-860f-086db4f277d6`
- Status characteristic: read
- RX characteristic: write-with-response
- TX characteristic: notify

Run it on a Linux host with BlueZ and a Bluetooth adapter:

```sh
cargo run --manifest-path tools/ble-gatt-spike/Cargo.toml
```

Expected behavior:

- The process powers the default adapter.
- It registers the Matchbox Control Service.
- It advertises as `Matchbox Audio`.
- Reads from Status return JSON transport metadata.
- Writes to RX are parsed as Matchbox BLE chunks.
- A complete chunked `system.hello` request is answered with a chunked response
  on TX if a client has subscribed.

## Target Image Check

The Raspberry Pi Zero 2 W Matchbox image is 32-bit ARM (`armv7l`). A quick
manual cross-build can use the Rust `arm-unknown-linux-gnueabihf` target and the
Yocto `dbus` sysroot component:

```sh
CARGO_TARGET_ARM_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc \
PKG_CONFIG_ALLOW_CROSS=1 \
PKG_CONFIG_SYSROOT_DIR="$PWD/yocto/build/tmp/sysroots-components/cortexa7t2hf-neon-vfpv4/dbus" \
PKG_CONFIG_PATH="$PWD/yocto/build/tmp/sysroots-components/cortexa7t2hf-neon-vfpv4/dbus/usr/lib/pkgconfig" \
RUSTFLAGS="-C link-arg=-Wl,-rpath-link=$PWD/yocto/build/tmp/sysroots-components/cortexa7t2hf-neon-vfpv4/systemd/usr/lib -C link-arg=-Wl,-rpath-link=$PWD/yocto/build/tmp/sysroots-components/cortexa7t2hf-neon-vfpv4/libcap/usr/lib -C link-arg=-Wl,-rpath-link=$PWD/yocto/build/tmp/sysroots-components/cortexa7t2hf-neon-vfpv4/zstd/usr/lib" \
cargo build --manifest-path tools/ble-gatt-spike/Cargo.toml --release --target arm-unknown-linux-gnueabihf
```

Copy and run it briefly:

```sh
scp tools/ble-gatt-spike/target/arm-unknown-linux-gnueabihf/release/ble-gatt-spike \
  matchbox@matchbox-audio.local:/tmp/ble-gatt-spike

ssh matchbox@matchbox-audio.local \
  'chmod +x /tmp/ble-gatt-spike; /tmp/ble-gatt-spike > /tmp/ble-gatt-spike.log 2>&1 & pid=$!; sleep 5; kill -INT $pid 2>/dev/null; wait $pid 2>/dev/null; cat /tmp/ble-gatt-spike.log'
```

Validation on May 16, 2026:

- Target: `matchbox@matchbox-audio.local`
- Architecture: `armv7l`
- Bluetooth service: active
- Adapter: `hci0`
- Result: GATT service registered and advertisement started as `Matchbox Audio`
  from the normal `matchbox` user.
