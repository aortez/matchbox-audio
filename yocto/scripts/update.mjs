#!/usr/bin/env node

import { basename, dirname, join } from 'path';
import { fileURLToPath } from 'url';

import {
  calculateChecksum,
  cleanupPreparedImage,
  colors,
  configureSSHKey,
  error,
  findLatestImage,
  formatBytes,
  info,
  loadConfig,
  log,
  prepareRootfs,
  remoteFlashWithKey,
  run,
  runCapture,
  success,
  transferImage,
  verifyRemoteChecksum,
  waitForReboot,
} from '../pi-base/scripts/lib/index.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
const YOCTO_DIR = dirname(__dirname);
const CONFIG_FILE = join(YOCTO_DIR, '.flash-config.json');
const IMAGE_DIR = join(YOCTO_DIR, 'build/tmp/deploy/images/raspberrypi0-2w');
const DEFAULT_HOST = 'matchbox-audio.local';
const DEFAULT_USER = 'matchbox';
const SSH_USERNAME = 'matchbox';
const REMOTE_UPDATE_DIR = '/data/matchbox-audio/update';
const PREFERRED_IMAGES = [
  'matchbox-audio-image-raspberrypi0-2w.rootfs.wic.gz',
  'matchbox-audio-image-raspberrypi0-2w.wic.gz',
];

function argValue(args, name, fallback) {
  const index = args.indexOf(name);
  if (index === -1) {
    return fallback;
  }
  return args[index + 1] || fallback;
}

function showHelp() {
  log(`
Matchbox Audio Remote Update

Usage:
  npm run update [options]

Options:
  --host <host>    Hostname or IP address (default: ${DEFAULT_HOST})
  --user <user>    SSH user (default: ${DEFAULT_USER})
  --skip-build     Reuse latest built image
  --dry-run        Show planned work without changing the device
  --yes            Skip final A/B update confirmation
  --no-wait        Do not wait for reboot after update
  -h, --help       Show this help
`);
}

async function ensureSshKeyConfig() {
  const config = loadConfig(CONFIG_FILE);
  if (config && config.ssh_key_path) {
    info(`Using SSH key: ${basename(config.ssh_key_path)}`);
    return config;
  }

  info('No SSH key configured yet.');
  return configureSSHKey(CONFIG_FILE);
}

async function main() {
  const args = process.argv.slice(2);

  if (args.includes('-h') || args.includes('--help')) {
    showHelp();
    return;
  }

  const host = argValue(args, '--host', DEFAULT_HOST);
  const user = argValue(args, '--user', DEFAULT_USER);
  const remoteTarget = `${user}@${host}`;
  const skipBuild = args.includes('--skip-build');
  const dryRun = args.includes('--dry-run');
  const skipConfirm = args.includes('--yes');
  const noWait = args.includes('--no-wait');

  log('');
  log(`${colors.bold}${colors.cyan}Matchbox Audio Remote Update${colors.reset}`);
  if (dryRun) {
    log(`${colors.yellow}(dry-run mode - no remote changes will be made)${colors.reset}`);
  }
  log('');
  info(`Target: ${remoteTarget}`);
  log('');

  const config = await ensureSshKeyConfig();

  if (!skipBuild) {
    info('Building image...');
    await run('npm', ['run', 'build'], { cwd: YOCTO_DIR });
  }

  const image = findLatestImage(IMAGE_DIR, '.wic.gz', PREFERRED_IMAGES);
  if (!image) {
    throw new Error('No image found. Run "npm run build" first.');
  }

  info(`Image: ${image.name}`);
  info(`Size: ${formatBytes(image.stat.size)}`);

  let prepared = null;
  try {
    if (dryRun) {
      info('Would prepare rootfs image from WIC.');
      return;
    }

    prepared = await prepareRootfs(image.path, null, SSH_USERNAME);
    const checksum = await calculateChecksum(prepared.preparedRootfsPath);

    const mkdirResult = runCapture(
      `ssh -o ConnectTimeout=10 -o BatchMode=yes ${remoteTarget} "mkdir -p ${REMOTE_UPDATE_DIR}"`,
    );
    if (mkdirResult === null) {
      throw new Error(`Failed to create remote update directory ${REMOTE_UPDATE_DIR}.`);
    }

    const transfer = await transferImage(
      prepared.preparedRootfsPath,
      checksum,
      remoteTarget,
      REMOTE_UPDATE_DIR,
    );

    if (!verifyRemoteChecksum(transfer.remoteImagePath, transfer.remoteChecksumPath, remoteTarget)) {
      throw new Error('Remote checksum verification failed.');
    }

    const remoteKeyPath = `${REMOTE_UPDATE_DIR}/${basename(config.ssh_key_path)}`;
    await run('scp', [
      '-o',
      'ConnectTimeout=10',
      '-o',
      'BatchMode=yes',
      config.ssh_key_path,
      `${remoteTarget}:${remoteKeyPath}`,
    ]);

    await remoteFlashWithKey(
      transfer.remoteImagePath,
      remoteKeyPath,
      SSH_USERNAME,
      remoteTarget,
      false,
      skipConfirm,
    );

    if (!noWait) {
      await waitForReboot(remoteTarget, host, 0, 180);
      runCapture(
        `ssh -o ConnectTimeout=10 -o BatchMode=yes ${remoteTarget} "rm -f ${transfer.remoteImagePath} ${transfer.remoteChecksumPath} ${remoteKeyPath}"`,
      );
    }

    success('Remote update complete.');
  } finally {
    if (prepared) {
      cleanupPreparedImage(prepared.workDir);
    }
  }
}

main().catch(err => {
  error(err.message);
  process.exit(1);
});
