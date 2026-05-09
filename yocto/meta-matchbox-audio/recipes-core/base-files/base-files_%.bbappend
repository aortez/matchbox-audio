# Mount the FAT boot partition at /boot so ab-boot-manager can update
# cmdline.txt during remote A/B updates. Mirrors dirtsim's project-specific
# fstab override, adapted for Pi Zero 2 W SD-card boot.
FILESEXTRAPATHS:prepend := "${THISDIR}/${PN}:"
SRC_URI += "file://fstab"

do_install:append() {
    install -m 0644 ${WORKDIR}/fstab ${D}${sysconfdir}/fstab
}
