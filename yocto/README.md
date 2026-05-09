# Matchbox Audio Yocto

This directory contains the Phase 1 Raspberry Pi image integration for Matchbox
Audio. It uses the pi-base layer and helper scripts from the
`sparkle-duck-shared` submodule at `yocto/pi-base`.

## Target

The first target is Raspberry Pi Zero 2 W:

```sh
npm run build
```

The build host must provide GNU coreutils. On Ubuntu variants where unprefixed
coreutils come from uutils, install `gnu-coreutils`; the build script will
automatically prefer the `gnu*` binaries for BitBake host tools.

The build uses `kas-matchbox-audio-zero2.yml` and creates:

- A/B root filesystems
- persistent `/data`
- NetworkManager, Avahi, BlueZ, and SSH
- `mba-player.service`
- `mba-cli`

The shared pi-base layer includes a BLE Wi-Fi provisioner by default, but this
image removes it for Phase 1. BlueZ remains installed; provisioning and car-mode
hotspot behavior are handled in later phases.

## Flash

Create optional home Wi-Fi credentials for the initial remote development loop:

```sh
cp wifi-creds.local.example wifi-creds.local
```

Then flash an SD card:

```sh
npm run flash
```

The flash script injects an SSH key, writes the hostname, optionally injects
home Wi-Fi credentials, and grows `/data` to fill the card while leaving 10%
unallocated. The checked-in `../config/hotspot.local.example.json` is reserved
for the Phase 2 car-mode hotspot work.

## Remote Smoke Test

After boot:

```sh
npm run smoke
```

By default this checks `matchbox@matchbox-audio.local` for:

- SSH reachability
- `mba-player.service` active state
- `mba-cli status`
- `/data` mount
- A/B slot status

Use another host or user with:

```sh
npm run smoke -- --host 192.168.1.42 --user matchbox
```

## Remote Update

After a device has already been flashed once:

```sh
npm run update
```

This prepares the root filesystem from the latest WIC image, transfers it to the
device, flashes the inactive rootfs via `ab-update-with-key`, and reboots.
