FILESEXTRAPATHS:prepend := "${THISDIR}/${PN}:"

SRC_URI += " \
    file://mpd.conf \
    file://matchbox-overrides.conf \
"

# Trim PACKAGECONFIG to the decoder set Matchbox Audio actually needs:
# ALSA output and the MP3/FLAC/Ogg formats listed in the requirements.
# Commercial-licensed codecs and unrelated transports are dropped to keep the
# image small and the build deterministic across LICENSE_FLAGS_ACCEPTED toggles.
PACKAGECONFIG = "alsa daemon flac mpg123 vorbis"

# The upstream recipe ships mpd.conf.in and runs sed substitutions to point
# state at /var/lib/mpd. Replace the rendered config with the Matchbox copy
# that targets /data/mpd, and install the systemd drop-in that orders MPD
# after /data is mounted and hardens the service.
do_install:append() {
    install -m 0644 ${WORKDIR}/mpd.conf ${D}${sysconfdir}/mpd.conf

    install -d ${D}${systemd_system_unitdir}/mpd.service.d
    install -m 0644 ${WORKDIR}/matchbox-overrides.conf \
        ${D}${systemd_system_unitdir}/mpd.service.d/matchbox-overrides.conf

    # The upstream /var/lib/mpd tree is unused on Matchbox; remove it so the
    # rootfs does not ship empty directories owned by mpd that imply a state
    # location that is not the canonical one.
    rm -rf ${D}${localstatedir}/lib/mpd

    # MPD's meson build installs mpd.socket alongside mpd.service. Matchbox
    # uses the long-running service unit instead of socket activation, so
    # drop the socket so bitbake's installed-vs-shipped QA stays clean and
    # the unit cannot be enabled by accident on the target.
    rm -f ${D}${systemd_system_unitdir}/mpd.socket
}

# Use the long-running service rather than socket activation. MPD's database
# load can take noticeable time on a Pi Zero 2 W; starting eagerly avoids
# making the first client wait.
SYSTEMD_SERVICE:${PN} = "mpd.service"

FILES:${PN} += " \
    ${systemd_system_unitdir}/mpd.service.d/matchbox-overrides.conf \
"
