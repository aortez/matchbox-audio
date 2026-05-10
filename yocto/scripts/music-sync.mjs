#!/usr/bin/env node

import { spawnSync } from 'child_process';
import { existsSync, readFileSync } from 'fs';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

import {
  colors,
  error,
  info,
  log,
  success,
  warn,
} from '../pi-base/scripts/lib/index.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
const YOCTO_DIR = dirname(__dirname);
const REPO_ROOT = dirname(YOCTO_DIR);

const DEFAULT_HOST = 'matchbox-audio.local';
const DEFAULT_USER = 'matchbox';
const DEFAULT_PORT = 8090;
const REMOTE_MUSIC_DIR = '/data/music/';
const CONFIG_PATH = join(REPO_ROOT, 'config', 'sync.local.json');
const RSYNC_EXCLUDES = [
  '.DS_Store',
  '._*',
  '.AppleDouble',
  '.Spotlight-V100',
  '.Trashes',
];

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
Matchbox Audio Music Sync

Pushes a local music library to the device via rsync, then triggers an MPD rescan.

Usage:
  npm run music-sync [-- options]

Options:
  --source <path>     Local source directory (default from config).
  --host <host>       Device hostname or IP (default from config or ${DEFAULT_HOST}).
  --target <host>     Alias for --host.
  --user <user>       SSH user (default from config or ${DEFAULT_USER}).
  --port <port>       Player API port (default ${DEFAULT_PORT}).
  --dry-run           Show what rsync would transfer without copying.
  --no-delete         Do not delete files on the device that are absent locally.
  -h, --help          Show this help.

Config file: ${CONFIG_PATH}
  {
    "source": "/path/to/music",
    "host": "${DEFAULT_HOST}",
    "user": "${DEFAULT_USER}"
  }
`);
}

function loadSyncConfig() {
  if (!existsSync(CONFIG_PATH)) {
    return {};
  }
  try {
    const raw = readFileSync(CONFIG_PATH, 'utf8');
    const parsed = JSON.parse(raw);
    if (typeof parsed !== 'object' || parsed === null) {
      throw new Error('config root must be an object');
    }
    return parsed;
  } catch (err) {
    throw new Error(`failed to parse ${CONFIG_PATH}: ${err.message}`);
  }
}

function ensureTrailingSlash(path) {
  return path.endsWith('/') ? path : `${path}/`;
}

function rsyncMusic({ source, user, host, dryRun, deleteRemote }) {
  const args = ['-av', '--human-readable'];
  if (dryRun) {
    args.push('--dry-run');
  }
  if (deleteRemote) {
    args.push('--delete');
  }
  for (const exclude of RSYNC_EXCLUDES) {
    args.push(`--exclude=${exclude}`);
  }
  args.push(ensureTrailingSlash(source));
  args.push(`${user}@${host}:${REMOTE_MUSIC_DIR}`);

  info(`rsync ${args.join(' ')}`);
  const result = spawnSync('rsync', args, { stdio: 'inherit' });
  if (result.status !== 0) {
    throw new Error(`rsync exited with status ${result.status ?? 'signal'}`);
  }
}

async function triggerRescan({ host, port }) {
  const url = `http://${host}:${port}/api/v1/library/rescan`;
  info(`POST ${url}`);
  const response = await fetch(url, { method: 'POST' });
  if (!response.ok) {
    const body = await response.text().catch(() => '');
    throw new Error(`rescan failed (${response.status}): ${body.trim()}`);
  }
  const payload = await response.json().catch(() => null);
  if (payload && typeof payload.job_id === 'number') {
    success(`rescan started: job ${payload.job_id}`);
  } else {
    success('rescan started.');
  }
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes('-h') || args.includes('--help')) {
    showHelp();
    return;
  }

  const config = loadSyncConfig();

  const source = argValue(args, '--source', config.source);
  const host = argValue(args, ['--host', '--target'], config.host || DEFAULT_HOST);
  const user = argValue(args, '--user', config.user || DEFAULT_USER);
  const port = Number(argValue(args, '--port', config.port || DEFAULT_PORT));
  const dryRun = args.includes('--dry-run');
  const deleteRemote = !args.includes('--no-delete');

  if (!source) {
    throw new Error(
      `no source directory configured. Set "source" in ${CONFIG_PATH} or pass --source <path>.`
    );
  }
  if (!existsSync(source)) {
    throw new Error(`source directory does not exist: ${source}`);
  }
  if (!Number.isFinite(port) || port <= 0 || port > 65535) {
    throw new Error(`invalid port: ${port}`);
  }

  log('');
  log(`${colors.bold}${colors.cyan}Matchbox Audio Music Sync${colors.reset}`);
  info(`Source: ${source}`);
  info(`Target: ${user}@${host}:${REMOTE_MUSIC_DIR}`);
  if (dryRun) {
    warn('Dry run — no files will be copied.');
  }
  if (!deleteRemote) {
    warn('Skipping --delete; remote files absent locally will be kept.');
  }
  log('');

  rsyncMusic({ source, user, host, dryRun, deleteRemote });

  if (dryRun) {
    info('Dry run complete; skipping rescan.');
    return;
  }

  await triggerRescan({ host, port });
}

main().catch(err => {
  error(err.message);
  process.exit(1);
});
