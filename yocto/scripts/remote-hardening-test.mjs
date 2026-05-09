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

  expectOk(remoteTarget, 'sudo allowlist is visible', 'sudo -n -l');
  expectFail(remoteTarget, 'plain sudo is denied', 'sudo -n true');
  expectFail(remoteTarget, 'shell sudo is denied', "sudo -n sh -c 'id'");
  expectFail(remoteTarget, 'shadow read is denied', 'sudo -n cat /etc/shadow');
  expectOk(remoteTarget, 'boot config helper is allowed', 'sudo -n /usr/bin/mba-boot-config ensure-pirate-audio');

  expectRemoteValue(remoteTarget, 'app data directory mode', 'stat -c %U:%G:%a /data/matchbox-audio', 'mba-player:matchbox-audio:711');
  expectRemoteValue(remoteTarget, 'network data directory mode', 'stat -c %U:%G:%a /data/matchbox-audio/network', 'root:matchbox-audio:750');
  expectRemoteValue(remoteTarget, 'update directory mode', 'stat -c %U:%G:%a /data/matchbox-audio/update', 'matchbox:matchbox:750');
  expectRemoteValue(remoteTarget, 'music directory mode', 'stat -c %U:%G:%a /data/music', 'matchbox:matchbox:755');
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
