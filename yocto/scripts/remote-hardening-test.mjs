#!/usr/bin/env node

import { spawnSync } from 'child_process';

import {
  colors,
  error,
  info,
  log,
  success,
} from '../pi-base/scripts/lib/index.mjs';

const DEFAULT_HOST = 'matchbox-audio.local';
const DEFAULT_USER = 'matchbox';

function argValue(args, names, fallback) {
  const aliases = Array.isArray(names) ? names : [names];

  for (const name of aliases) {
    const index = args.indexOf(name);
    if (index !== -1) {
      return args[index + 1] || fallback;
    }

    const prefix = `${name}=`;
    const inline = args.find(arg => arg.startsWith(prefix));
    if (inline) {
      return inline.slice(prefix.length) || fallback;
    }
  }

  return fallback;
}

function showHelp() {
  log(`
Matchbox Audio Remote Hardening Test

Usage:
  npm run hardening [options]

Options:
  --host <host>     Hostname or IP address (default: ${DEFAULT_HOST})
  --target <host>   Alias for --host
  --user <user>     SSH user (default: ${DEFAULT_USER})
  -h, --help        Show this help
`);
}

function ssh(remoteTarget, command) {
  return spawnSync(
    'ssh',
    ['-o', 'ConnectTimeout=10', '-o', 'BatchMode=yes', remoteTarget, command],
    { encoding: 'utf8' },
  );
}

function commandText(result) {
  return [result.stdout.trim(), result.stderr.trim()].filter(Boolean).join('\n');
}

function check(name, action) {
  info(name);
  const output = action();
  if (output) {
    log(output);
  }
  success(`${name} OK`);
  log('');
}

function expectOk(remoteTarget, name, command) {
  check(name, () => {
    const result = ssh(remoteTarget, command);
    if (result.status !== 0) {
      throw new Error(commandText(result) || `${command} failed`);
    }
    return result.stdout.trim();
  });
}

function expectFail(remoteTarget, name, command) {
  check(name, () => {
    const result = ssh(remoteTarget, command);
    if (result.status === 0) {
      throw new Error(`${command} unexpectedly succeeded`);
    }
    return commandText(result);
  });
}

function expectRemoteValue(remoteTarget, name, command, expected) {
  expectOk(remoteTarget, name, `[ "$(${command})" = "${expected}" ]`);
}

function main() {
  const args = process.argv.slice(2);

  if (args.includes('-h') || args.includes('--help')) {
    showHelp();
    return;
  }

  const host = argValue(args, ['--host', '--target'], DEFAULT_HOST);
  const user = argValue(args, '--user', DEFAULT_USER);
  const remoteTarget = `${user}@${host}`;

  log('');
  log(`${colors.bold}${colors.cyan}Matchbox Audio Remote Hardening Test${colors.reset}`);
  log('');
  info(`Target: ${remoteTarget}`);
  log('');

  expectOk(remoteTarget, 'SSH reachability', 'echo reachable');
  expectOk(remoteTarget, 'mba-player service active', 'systemctl is-active mba-player.service');
  expectOk(remoteTarget, 'mba-device service active', 'systemctl is-active mba-device.service');
  expectOk(remoteTarget, 'mpd service active', 'systemctl is-active mpd.service');
  expectOk(remoteTarget, 'mpd startup volume service active', 'systemctl is-active mba-mpd-startup-volume.service');
  expectOk(remoteTarget, 'network restore service active', 'systemctl is-active mba-network-mode-restore.service');
  expectOk(remoteTarget, 'mba-cli status', 'mba-cli status');

  check('public API redacts hotspot password', () => {
    const result = ssh(remoteTarget, 'curl -fsS http://127.0.0.1:8090/api/v1/status');
    if (result.status !== 0) {
      throw new Error(commandText(result) || 'status API request failed');
    }
    if (result.stdout.includes('hotspot_password')) {
      throw new Error('/api/v1/status exposed hotspot_password');
    }
    JSON.parse(result.stdout);
    return result.stdout.trim();
  });

  expectOk(
    remoteTarget,
    'public network status is unprivileged and redacted',
    'mba-network-mode status | tee /tmp/mba-network-status && ! grep -q hotspot_password /tmp/mba-network-status && rm -f /tmp/mba-network-status',
  );
  expectFail(remoteTarget, 'display status is not readable by matchbox', 'mba-network-mode display-status');

  expectRemoteValue(remoteTarget, 'mba-player service user', 'systemctl show -p User --value mba-player.service', 'mba-player');
  expectRemoteValue(remoteTarget, 'mba-player service group', 'systemctl show -p Group --value mba-player.service', 'matchbox-audio');
  expectRemoteValue(remoteTarget, 'mba-player no-new-privileges', 'systemctl show -p NoNewPrivileges --value mba-player.service', 'yes');

  expectRemoteValue(remoteTarget, 'mpd no-new-privileges', 'systemctl show -p NoNewPrivileges --value mpd.service', 'yes');
  expectRemoteValue(remoteTarget, 'mpd protect system', 'systemctl show -p ProtectSystem --value mpd.service', 'strict');
  expectRemoteValue(remoteTarget, 'mpd startup volume service user', 'systemctl show -p User --value mba-mpd-startup-volume.service', 'mpd');
  expectRemoteValue(remoteTarget, 'mpd startup volume service group', 'systemctl show -p Group --value mba-mpd-startup-volume.service', 'audio');
  expectRemoteValue(remoteTarget, 'mpd startup volume no-new-privileges', 'systemctl show -p NoNewPrivileges --value mba-mpd-startup-volume.service', 'yes');
  expectRemoteValue(
    remoteTarget,
    'mpd binds 127.0.0.1:6600 only',
    "netstat -lnt | awk '$4 ~ /:6600$/ {print $4}' | sort -u | tr '\\n' ' ' | sed 's/ $//'",
    '127.0.0.1:6600',
  );
  expectOk(remoteTarget, 'mpc status reaches mpd', 'mpc status >/dev/null');
  expectOk(remoteTarget, 'aplay is installed', 'command -v aplay >/dev/null');
  expectOk(remoteTarget, 'PIM483 boot overlay is configured', "grep -Fxq 'dtoverlay=hifiberry-dac' /boot/config.txt && grep -Fxq 'dtparam=audio=off' /boot/config.txt");
  expectOk(remoteTarget, 'PIM483 ALSA card is visible', "grep -qi hifiberry /proc/asound/cards && aplay -l | grep -qi hifiberry");
  expectOk(remoteTarget, 'ALSA default targets Matchbox I2S card', "grep -q 'pcm.matchbox_i2s_hw' /etc/asound.conf");
  expectOk(remoteTarget, 'MPD ALSA output is configured', "mpc outputs | grep -q 'matchbox-pim483-lineout'");

  expectOk(remoteTarget, 'sudo allowlist is visible', 'sudo -n -l');
  expectFail(remoteTarget, 'plain sudo is denied', 'sudo -n true');
  expectFail(remoteTarget, 'shell sudo is denied', "sudo -n sh -c 'id'");
  expectFail(remoteTarget, 'shadow read is denied', 'sudo -n cat /etc/shadow');
  expectFail(remoteTarget, 'sudo journalctl is denied', 'sudo -n journalctl -n 1');
  expectOk(remoteTarget, 'journal is readable without sudo', 'id -nG | grep -qw systemd-journal && journalctl --no-pager -n 1 >/dev/null');
  expectOk(remoteTarget, 'ALSA diagnostics are readable by matchbox', 'id -nG | grep -qw audio && aplay -l >/dev/null');
  expectOk(remoteTarget, 'boot config helper is allowed', 'sudo -n /usr/bin/mba-boot-config ensure-pirate-audio');
  expectOk(remoteTarget, 'mba-ab-update wrapper is allowed', 'sudo -n -l /usr/bin/mba-ab-update >/dev/null');
  expectFail(remoteTarget, 'direct ab-update-with-key is denied', 'sudo -n -l /usr/sbin/ab-update-with-key /data/matchbox-audio/update/probe /data/matchbox-audio/update/key.pub matchbox >/dev/null 2>&1');
  expectOk(
    remoteTarget,
    'mba-ab-update rejects 4-arg whitespace bypass',
    'touch /data/matchbox-audio/update/_probe-image /data/matchbox-audio/update/_probe-key && '
      + 'out=$(sudo -n /usr/bin/mba-ab-update /data/matchbox-audio/update/_probe-image /data/matchbox-audio/update/_probe-key root matchbox 2>&1); rc=$?; '
      + 'rm -f /data/matchbox-audio/update/_probe-image /data/matchbox-audio/update/_probe-key; '
      + '[ "$rc" -ne 0 ] && echo "$out" | grep -q usage',
  );
  expectOk(
    remoteTarget,
    'mba-ab-update rejects wrong username',
    'touch /data/matchbox-audio/update/_probe-image /data/matchbox-audio/update/_probe-key && '
      + 'out=$(sudo -n /usr/bin/mba-ab-update /data/matchbox-audio/update/_probe-image /data/matchbox-audio/update/_probe-key root 2>&1); rc=$?; '
      + 'rm -f /data/matchbox-audio/update/_probe-image /data/matchbox-audio/update/_probe-key; '
      + '[ "$rc" -ne 0 ] && echo "$out" | grep -q "username must be"',
  );
  expectOk(
    remoteTarget,
    'mba-ab-update rejects path traversal',
    'out=$(sudo -n /usr/bin/mba-ab-update /data/matchbox-audio/update/../etc/passwd /data/matchbox-audio/update/_probe-key 2>&1); rc=$?; '
      + '[ "$rc" -ne 0 ] && echo "$out" | grep -qE "(resolves outside|not a regular file)"',
  );
  expectOk(
    remoteTarget,
    'mba-ab-update rejects paths outside update dir',
    'out=$(sudo -n /usr/bin/mba-ab-update /etc/passwd /etc/hostname 2>&1); rc=$?; '
      + '[ "$rc" -ne 0 ] && echo "$out" | grep -q "resolves outside"',
  );

  expectRemoteValue(remoteTarget, 'app data directory mode', 'stat -c %U:%G:%a /data/matchbox-audio', 'root:root:711');
  expectRemoteValue(remoteTarget, 'app state directory mode', 'stat -c %U:%G:%a /data/matchbox-audio/state', 'mba-player:matchbox-audio:750');
  expectRemoteValue(remoteTarget, 'network data directory mode', 'stat -c %U:%G:%a /data/matchbox-audio/network', 'root:matchbox-audio:750');
  expectRemoteValue(remoteTarget, 'update directory mode', 'stat -c %U:%G:%a /data/matchbox-audio/update', 'matchbox:matchbox:750');
  expectRemoteValue(remoteTarget, 'music directory mode', 'stat -c %U:%G:%a /data/music', 'matchbox:matchbox:755');
  expectRemoteValue(remoteTarget, 'mpd directory mode', 'stat -c %U:%G:%a /data/mpd', 'mpd:mpd:750');
  // The 0750 mode on /data/mpd intentionally hides everything inside from
  // the matchbox user, so subdirectory ownership cannot be verified from this
  // SSH session. The live mpc status check below proves MPD owns the tree.
  expectFail(remoteTarget, 'hotspot secret is not readable by matchbox', 'test -r /data/matchbox-audio/network/hotspot.env');
  expectOk(
    remoteTarget,
    'music directory writable by matchbox',
    'mkdir -p /data/music/_hardening-test && echo ok > /data/music/_hardening-test/probe.txt && rm -rf /data/music/_hardening-test',
  );
  expectOk(
    remoteTarget,
    'update directory writable by matchbox',
    'mkdir -p /data/matchbox-audio/update/_hardening-test && echo ok > /data/matchbox-audio/update/_hardening-test/probe.txt && rm -rf /data/matchbox-audio/update/_hardening-test',
  );

  success('Remote hardening test passed.');
}

try {
  main();
} catch (err) {
  error(err.message);
  process.exit(1);
}
