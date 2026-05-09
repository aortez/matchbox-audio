SUMMARY = "Matchbox Audio image"
DESCRIPTION = "Minimal Raspberry Pi image for the Matchbox Audio player."
LICENSE = "MIT"

inherit pi-base-image extrausers

BOOT_DEVICE = "mmcblk0"
HOSTNAME_DEFAULT = "matchbox-audio"

# Temporary Phase 1 HDMI-console recovery password for user `matchbox`:
# `matchbox`. Keep SSH password auth disabled below so this is local-console only.
MATCHBOX_CONSOLE_PASSWORD_HASH = "\$6\$matchbox\$KH1Y8n6bm.xLZ6D.8cyfVT8bjNQBYqRyl201wSMSYm9v/Emm0VEpaiW6go.AZvqRAD51a.CrDNA7GM2DDRQYf0"

EXTRA_USERS_PARAMS = " \
    useradd -m -u 1000 -s /bin/sh -G sudo matchbox; \
    usermod -p '${MATCHBOX_CONSOLE_PASSWORD_HASH}' matchbox; \
"

setup_matchbox_ssh() {
    install -d -m 700 ${IMAGE_ROOTFS}/home/matchbox/.ssh
    touch ${IMAGE_ROOTFS}/home/matchbox/.ssh/authorized_keys
    chmod 600 ${IMAGE_ROOTFS}/home/matchbox/.ssh/authorized_keys
    chown -R 1000:1000 ${IMAGE_ROOTFS}/home/matchbox/.ssh
}
ROOTFS_POSTPROCESS_COMMAND:append = " setup_matchbox_ssh;"

setup_matchbox_sudo() {
    install -d -m 755 ${IMAGE_ROOTFS}/etc/sudoers.d
    echo "matchbox ALL=(ALL) NOPASSWD: ALL" > ${IMAGE_ROOTFS}/etc/sudoers.d/matchbox
    chmod 440 ${IMAGE_ROOTFS}/etc/sudoers.d/matchbox
}
ROOTFS_POSTPROCESS_COMMAND:append = " setup_matchbox_sudo;"

setup_matchbox_sshd_policy() {
    install -d -m 755 ${IMAGE_ROOTFS}/etc/ssh/sshd_config.d
    cat > ${IMAGE_ROOTFS}/etc/ssh/sshd_config.d/99-matchbox-audio.conf <<'EOF'
PasswordAuthentication no
KbdInteractiveAuthentication no
PubkeyAuthentication yes
PermitRootLogin no
EOF
}
ROOTFS_POSTPROCESS_COMMAND:append = " setup_matchbox_sshd_policy;"

# Pi Zero 2 W Wi-Fi firmware and NetworkManager Wi-Fi support for the initial
# home-network remote development loop.
IMAGE_INSTALL:append = " \
    linux-firmware-rpidistro-bcm43436 \
    networkmanager-wifi \
    openssh-sftp-server \
    kbd \
"

# BLE provisioning is deferred. Keep BlueZ from pi-base, but avoid pulling in
# the provisioner daemon and Yocto's source-built Rust stack for Phase 1. The
# inherited nmtui package is not available in this layer set; nmcli is enough
# for the Phase 1 remote setup loop.
IMAGE_INSTALL:remove = "wifi-provisioner networkmanager-nmtui"

IMAGE_INSTALL:append = " \
    matchbox-audio \
"
