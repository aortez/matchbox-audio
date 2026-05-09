# Matchbox Audio Yocto

This directory contains the Phase 1 Raspberry Pi image integration for Matchbox
Audio. It uses the pi-base layer and helper scripts from the
`sparkle-duck-shared` submodule at `yocto/pi-base`.

## Target

The first target is Raspberry Pi Zero 2 W:

```sh
npm run build
```

Remote updates only need a rootfs payload, so the update script uses:

```sh
npm run build -- --update-payload
```

The build host must provide GNU coreutils. On Ubuntu variants where unprefixed
coreutils come from uutils, install `gnu-coreutils`; the build script will
automatically prefer the `gnu*` binaries for BitBake host tools.

The build uses `kas-matchbox-audio-zero2.yml` and creates:

- A/B root filesystems
- persistent `/data`
- a standalone `ext4.gz` rootfs payload for remote A/B updates
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

For Phase 1 bring-up only, the HDMI console accepts `matchbox` / `matchbox`.
SSH remains key-only; password login is disabled in `sshd_config.d`.

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
../update.sh --skip-build --smoke
```

The top-level wrapper defaults to `matchbox-audio.local` and skips the final
flash confirmation, matching the dirtsim update wrapper. Use `--confirm` if you
want the interactive prompt.

The updater builds and prefers the standalone `ext4.gz` rootfs payload, avoiding
full WIC SD-card image creation for the remote update path. If only a WIC image
is available, it extracts the rootfs payload locally without sudo. Before
transfer it checks SSH reachability, verifies `/data` is mounted as ext4,
prepares `/data/matchbox-audio/update`, checks available space, and verifies
`ab-boot-manager` on the target. It then transfers the payload, verifies the
checksum, bootstraps `ab-update-with-key` if needed, flashes the inactive slot,
injects the configured SSH key, reboots, and optionally runs the smoke test.

Useful variants:

```sh
../update.sh --target 192.168.1.42 --skip-build
../update.sh --target matchbox-audio.local --skip-build --smoke
npm run update -- --host matchbox-audio.local --skip-build --yes
```
