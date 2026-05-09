SUMMARY = "Matchbox Audio daemon and CLI"
DESCRIPTION = "Rust daemon and command-line client for Matchbox Audio."
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COMMON_LICENSE_DIR}/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

inherit externalsrc cargo_bin systemd

MATCHBOX_AUDIO_SRCROOT = "${@os.path.realpath('${THISDIR}/../../../..')}"
# Keep EXTERNALSRC below the repo root because the Yocto build tree also lives
# in this repository; otherwise pseudo ignores its own package staging paths.
EXTERNALSRC = "${MATCHBOX_AUDIO_SRCROOT}/crates"
CARGO_MANIFEST_PATH = "${MATCHBOX_AUDIO_SRCROOT}/Cargo.toml"
EXTRA_CARGO_FLAGS = "--locked --workspace"
EXTRA_RUSTFLAGS += "--remap-path-prefix=${WORKDIR}=${TARGET_DBGSRC_DIR}"

# The 32-bit ARM Rust libc stack still trips Yocto's time64 QA heuristic.
# Yocto's cargo_common class applies the same skip for source-built Rust crates.
INSANE_SKIP += "32bit-time"
INSANE_SKIP:${PN}-dbg += "buildpaths"

do_compile[network] = "1"

python () {
    srcroot = d.getVar("MATCHBOX_AUDIO_SRCROOT")
    d.appendVarFlag("do_compile", "file-checksums", f" {srcroot}/Cargo.toml:True {srcroot}/Cargo.lock:True")
    d.appendVarFlag(
        "do_install",
        "file-checksums",
        " "
        + " ".join(
            [
                f"{srcroot}/yocto/meta-matchbox-audio/recipes-matchbox-audio/matchbox-audio/files/mba-ab-update:True",
                f"{srcroot}/yocto/meta-matchbox-audio/recipes-matchbox-audio/matchbox-audio/files/mba-boot-config:True",
                f"{srcroot}/yocto/meta-matchbox-audio/recipes-matchbox-audio/matchbox-audio/files/mba-data-init:True",
                f"{srcroot}/yocto/meta-matchbox-audio/recipes-matchbox-audio/matchbox-audio/files/mba-data-init.service:True",
                f"{srcroot}/yocto/meta-matchbox-audio/recipes-matchbox-audio/matchbox-audio/files/mba-device.service:True",
                f"{srcroot}/yocto/meta-matchbox-audio/recipes-matchbox-audio/matchbox-audio/files/mba-mpd-startup-volume:True",
                f"{srcroot}/yocto/meta-matchbox-audio/recipes-matchbox-audio/matchbox-audio/files/mba-mpd-startup-volume.service:True",
                f"{srcroot}/yocto/meta-matchbox-audio/recipes-matchbox-audio/matchbox-audio/files/mba-network-mode:True",
                f"{srcroot}/yocto/meta-matchbox-audio/recipes-matchbox-audio/matchbox-audio/files/mba-network-mode-restore.service:True",
                f"{srcroot}/yocto/meta-matchbox-audio/recipes-matchbox-audio/matchbox-audio/files/mba-player.service:True",
            ]
        ),
    )
}

do_install() {
    install -d ${D}${bindir}
    install -m 0755 ${CARGO_BINDIR}/mba-player ${D}${bindir}/mba-player
    install -m 0755 ${CARGO_BINDIR}/mba-cli ${D}${bindir}/mba-cli
    install -m 0755 ${CARGO_BINDIR}/mba-device ${D}${bindir}/mba-device
    install -m 0755 ${THISDIR}/files/mba-ab-update ${D}${bindir}/mba-ab-update
    install -m 0755 ${THISDIR}/files/mba-boot-config ${D}${bindir}/mba-boot-config
    install -m 0755 ${THISDIR}/files/mba-mpd-startup-volume ${D}${bindir}/mba-mpd-startup-volume
    install -m 0755 ${THISDIR}/files/mba-network-mode ${D}${bindir}/mba-network-mode
    install -d ${D}${sbindir}
    install -m 0755 ${THISDIR}/files/mba-data-init ${D}${sbindir}/mba-data-init

    install -d ${D}${systemd_system_unitdir}
    install -m 0644 ${THISDIR}/files/mba-data-init.service ${D}${systemd_system_unitdir}/mba-data-init.service
    install -m 0644 ${THISDIR}/files/mba-player.service ${D}${systemd_system_unitdir}/mba-player.service
    install -m 0644 ${THISDIR}/files/mba-device.service ${D}${systemd_system_unitdir}/mba-device.service
    install -m 0644 ${THISDIR}/files/mba-mpd-startup-volume.service ${D}${systemd_system_unitdir}/mba-mpd-startup-volume.service
    install -m 0644 ${THISDIR}/files/mba-network-mode-restore.service ${D}${systemd_system_unitdir}/mba-network-mode-restore.service
}

SYSTEMD_SERVICE:${PN} = "mba-data-init.service mba-network-mode-restore.service mba-mpd-startup-volume.service mba-player.service mba-device.service"
SYSTEMD_AUTO_ENABLE = "enable"

FILES:${PN} = " \
    ${bindir}/mba-player \
    ${bindir}/mba-cli \
    ${bindir}/mba-device \
    ${bindir}/mba-ab-update \
    ${bindir}/mba-boot-config \
    ${bindir}/mba-mpd-startup-volume \
    ${bindir}/mba-network-mode \
    ${sbindir}/mba-data-init \
    ${systemd_system_unitdir}/mba-data-init.service \
    ${systemd_system_unitdir}/mba-player.service \
    ${systemd_system_unitdir}/mba-device.service \
    ${systemd_system_unitdir}/mba-mpd-startup-volume.service \
    ${systemd_system_unitdir}/mba-network-mode-restore.service \
"
