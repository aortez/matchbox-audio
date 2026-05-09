#!/usr/bin/env node

import {
  colors,
  error,
  info,
  log,
  runCapture,
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
Matchbox Audio Remote Smoke Test

Usage:
  npm run smoke [options]

Options:
  --host <host>     Hostname or IP address (default: ${DEFAULT_HOST})
  --target <host>   Alias for --host, matching update.sh
  --user <user>     SSH user (default: ${DEFAULT_USER})
  -h, --help        Show this help
`);
}

function ssh(remoteTarget, command) {
  return runCapture(`ssh -o ConnectTimeout=5 -o BatchMode=yes ${remoteTarget} "${command}"`);
}

function check(name, action) {
  info(name);
  const output = action();
  if (output === null) {
    throw new Error(`${name} failed`);
  }
  if (output) {
    log(output);
  }
  success(`${name} OK`);
  log('');
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
  log(`${colors.bold}${colors.cyan}Matchbox Audio Remote Smoke Test${colors.reset}`);
  log('');
  info(`Target: ${remoteTarget}`);
  log('');

  check('SSH reachability', () => ssh(remoteTarget, 'echo reachable'));
  check('mba-player service', () => ssh(remoteTarget, 'systemctl is-active mba-player.service'));
  check('mba-cli status', () => ssh(remoteTarget, 'mba-cli status'));
  check('/data mount', () => ssh(remoteTarget, "grep ' /data ' /proc/mounts"));
  check('A/B slot status', () => ssh(remoteTarget, 'ab-boot-manager status'));

  success('Remote smoke test passed.');
}

try {
  main();
} catch (err) {
  error(err.message);
  process.exit(1);
}
