SUMMARY = "Matchbox Audio image"
DESCRIPTION = "Minimal Raspberry Pi image for the Matchbox Audio player."
LICENSE = "MIT"

inherit pi-base-image extrausers

BOOT_DEVICE = "mmcblk0"
HOSTNAME_DEFAULT = "matchbox-audio"

EXTRA_USERS_PARAMS = " \
    groupadd -g 1000 matchbox; \
    groupadd -r matchbox-audio; \
    useradd -m -u 1000 -g matchbox -G systemd-journal -s /bin/sh matchbox; \
    usermod -p '*' matchbox; \
    useradd -r -g matchbox-audio -d /nonexistent -s /bin/false mba-player; \
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
    install -m 0440 ${THISDIR}/files/matchbox-sudoers ${IMAGE_ROOTFS}/etc/sudoers.d/matchbox
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

setup_matchbox_network_policy() {
    install -d -m 755 ${IMAGE_ROOTFS}/etc/systemd/system
    rm -f ${IMAGE_ROOTFS}/etc/systemd/system/multi-user.target.wants/dnsmasq.service
    ln -snf /dev/null ${IMAGE_ROOTFS}/etc/systemd/system/dnsmasq.service
}
ROOTFS_POSTPROCESS_COMMAND:append = " setup_matchbox_network_policy;"

# Pi Zero 2 W Wi-Fi firmware and NetworkManager Wi-Fi support for the initial
# home-network remote development loop.
IMAGE_INSTALL:append = " \
    linux-firmware-rpidistro-bcm43436 \
    networkmanager-wifi \
    openssh-sftp-server \
    kbd \
    rsync \
"

# BLE provisioning is deferred. Keep BlueZ from pi-base, but avoid pulling in
# the provisioner daemon and Yocto's source-built Rust stack for Phase 1.
IMAGE_INSTALL:remove = "wifi-provisioner"

IMAGE_INSTALL:append = " \
    matchbox-audio \
"
