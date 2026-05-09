#!/usr/bin/env node

import { spawnSync } from 'child_process';
import { createReadStream, createWriteStream, existsSync, mkdtempSync, readdirSync, rmSync, statSync } from 'fs';
import { tmpdir } from 'os';
import { basename, dirname, join } from 'path';
import { pipeline } from 'stream/promises';
import { fileURLToPath } from 'url';
import { createGunzip, createGzip } from 'zlib';

import {
  calculateChecksum,
  colors,
  configureSSHKey,
  error,
  findLatestImage,
  formatBytes,
  getRemoteTmpSpace,
  info,
  loadConfig,
  log,
  remoteFlashWithKey,
  run,
  runCapture,
  ssh,
  success,
  transferImage,
  verifyRemoteChecksum,
  warn,
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
const REMOTE_UPDATE_HEADROOM_BYTES = 64 * 1024 * 1024;
const PIRATE_AUDIO_BOOT_CONFIG_LINES = [
  '# Matchbox Audio Pirate Audio Line Out',
  'dtparam=spi=on',
  'dtoverlay=hifiberry-dac',
  'gpio=25=op,dh',
  'dtparam=audio=off',
];
const LOCAL_UPDATE_SCRIPT = join(
  YOCTO_DIR,
  'pi-base/yocto/meta-pi-base/recipes-support/ab-boot/files/ab-update-with-key',
);
const PREFERRED_ROOTFS_IMAGES = [
  'matchbox-audio-image-raspberrypi0-2w.rootfs.ext4.gz',
  'matchbox-audio-image-raspberrypi0-2w.ext4.gz',
];
const PREFERRED_WIC_IMAGES = [
  'matchbox-audio-image-raspberrypi0-2w.rootfs.wic.gz',
  'matchbox-audio-image-raspberrypi0-2w.wic.gz',
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
Matchbox Audio Remote Update

Usage:
  npm run update [options]
  ./update.sh [options]

Options:
  --host <host>       Hostname or IP address (default: ${DEFAULT_HOST})
  --target <host>     Alias for --host, matching the dirtsim wrapper
  --user <user>       SSH user (default: ${DEFAULT_USER})
  --skip-build        Reuse latest built image
  --dry-run           Show planned work without changing the device
  --yes               Skip final A/B update confirmation
  --no-wait           Do not wait for reboot after update
  --smoke             Run the remote smoke test after reboot
  -h, --help          Show this help
`);
}

function shellQuote(value) {
  return `'${String(value).replaceAll("'", "'\\''")}'`;
}

function findLatestImageInDirs(dirs, suffix, preferredNames = []) {
  const files = [];

  for (const dir of dirs) {
    if (!existsSync(dir)) {
      continue;
    }

    for (const name of readdirSync(dir)) {
      if (!name.endsWith(suffix) || name.includes('->')) {
        continue;
      }

      const path = join(dir, name);
      files.push({ name, path, stat: statSync(path) });
    }
  }

  files.sort((a, b) => b.stat.mtimeMs - a.stat.mtimeMs);

  for (const preferred of preferredNames) {
    const found = files.find(file => file.name === preferred);
    if (found) {
      return found;
    }
  }

  return files[0] || null;
}

function findImageWorkDeployDirs() {
  const deployRoot = join(YOCTO_DIR, 'build/tmp/work');
  const matches = [];

  function visit(dir, depth) {
    if (depth > 6 || !existsSync(dir)) {
      return;
    }

    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (!entry.isDirectory()) {
        continue;
      }

      if (entry.name === 'deploy-matchbox-audio-image-image-complete') {
        matches.push(path);
      } else {
        visit(path, depth + 1);
      }
    }
  }

  visit(deployRoot, 0);
  return matches;
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

function runJson(command, args) {
  const result = spawnSync(command, args, {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  if (result.status !== 0) {
    const stderr = result.stderr.trim();
    throw new Error(`${command} ${args.join(' ')} failed${stderr ? `: ${stderr}` : ''}`);
  }

  return JSON.parse(result.stdout);
}

function rootfsPartitionFromWic(wicPath) {
  const partitionTable = runJson('sfdisk', ['--json', wicPath]).partitiontable;
  const rootfs = partitionTable?.partitions?.[1];
  const sectorSize = partitionTable?.sectorsize || 512;

  if (!rootfs || !Number.isInteger(rootfs.start) || !Number.isInteger(rootfs.size)) {
    throw new Error('Could not find rootfs A partition in WIC image.');
  }

  const start = rootfs.start * sectorSize;
  const size = rootfs.size * sectorSize;
  const end = start + size - 1;
  const imageSize = statSync(wicPath).size;

  if (start < 0 || size <= 0 || end >= imageSize) {
    throw new Error('WIC rootfs partition bounds are invalid.');
  }

  return { start, end, size };
}

async function prepareRootfsForRemoteUpdate(imagePath) {
  const workDir = mkdtempSync(join(tmpdir(), 'matchbox-audio-rootfs-'));
  const wicPath = join(workDir, 'image.wic');
  const rootfsRawPath = join(workDir, 'rootfs.ext4');
  const preparedRootfsPath = join(workDir, 'rootfs.ext4.gz');

  try {
    info('Decompressing image...');
    await pipeline(
      createReadStream(imagePath),
      createGunzip(),
      createWriteStream(wicPath),
    );

    const rootfs = rootfsPartitionFromWic(wicPath);
    info(`Extracting rootfs A partition (${formatBytes(rootfs.size)})...`);
    await pipeline(
      createReadStream(wicPath, { start: rootfs.start, end: rootfs.end }),
      createWriteStream(rootfsRawPath),
    );

    info('Compressing rootfs...');
    await pipeline(
      createReadStream(rootfsRawPath),
      createGzip(),
      createWriteStream(preparedRootfsPath),
    );

    rmSync(wicPath, { force: true });
    rmSync(rootfsRawPath, { force: true });

    success('Rootfs prepared.');
    return { preparedRootfsPath, workDir };
  } catch (err) {
    rmSync(workDir, { recursive: true, force: true });
    throw err;
  }
}

function findUpdateImage() {
  const rootfs = findLatestImageInDirs(
    [IMAGE_DIR, ...findImageWorkDeployDirs()],
    '.ext4.gz',
    PREFERRED_ROOTFS_IMAGES,
  );
  const wic = findLatestImage(IMAGE_DIR, '.wic.gz', PREFERRED_WIC_IMAGES);

  if (rootfs && wic && rootfs.stat.mtimeMs + 5 * 60 * 1000 < wic.stat.mtimeMs) {
    warn('Standalone rootfs is older than the latest WIC image; using WIC-derived rootfs.');
    return { ...wic, kind: 'wic' };
  }

  if (rootfs) {
    return { ...rootfs, kind: 'rootfs' };
  }

  if (wic) {
    return { ...wic, kind: 'wic' };
  }

  return null;
}

async function prepareUpdatePayload(image) {
  if (image.kind === 'rootfs') {
    info('Using standalone rootfs artifact.');
    return { preparedRootfsPath: image.path, workDir: null };
  }

  return prepareRootfsForRemoteUpdate(image.path);
}

function requireRemoteOk(remoteTarget, command, failureMessage) {
  const result = spawnSync(
    'ssh',
    ['-o', 'ConnectTimeout=10', '-o', 'BatchMode=yes', remoteTarget, command],
    { encoding: 'utf8' },
  );

  if (result.status !== 0) {
    const stderr = result.stderr.trim();
    throw new Error(stderr ? `${failureMessage}: ${stderr}` : failureMessage);
  }

  return result.stdout.trim();
}

function ensureRemoteBootConfig(remoteTarget) {
  info('Checking Pirate Audio boot config...');
  const helper = requireRemoteOk(
    remoteTarget,
    'command -v mba-boot-config || true',
    'Could not check for mba-boot-config on the remote device.',
  );
  if (helper) {
    const output = requireRemoteOk(
      remoteTarget,
      `sudo -n ${shellQuote(helper)} ensure-pirate-audio`,
      'Could not update /boot/config.txt for Pirate Audio hardware.',
    );
    if (output.includes('changed=1')) {
      success('Pirate Audio boot config updated.');
    } else {
      success('Pirate Audio boot config already present.');
    }
    return;
  }

  warn('Remote mba-boot-config helper not found; using legacy sudo boot-config update.');
  const quotedLines = PIRATE_AUDIO_BOOT_CONFIG_LINES
    .map(line => shellQuote(line))
    .join(' ');
  const command = `
set -eu
config=/boot/config.txt
sudo -n test -f "$config"
sudo -n test -w "$config"
changed=0
for line in ${quotedLines}; do
  if ! sudo -n grep -Fxq "$line" "$config"; then
    printf '%s\\n' "$line" | sudo -n tee -a "$config" >/dev/null
    changed=1
  fi
done
sudo -n sync
echo "$changed"
`;
  const changed = requireRemoteOk(
    remoteTarget,
    command,
    'Could not update /boot/config.txt for Pirate Audio hardware.',
  );

  if (changed === '1') {
    success('Pirate Audio boot config updated.');
  } else {
    success('Pirate Audio boot config already present.');
  }
}

function preflightRemote(remoteTarget) {
  info('Checking SSH reachability...');
  const reachable = ssh(remoteTarget, 'echo ok', { timeout: 10 });
  if (reachable !== 'ok') {
    throw new Error(`Cannot reach ${remoteTarget} over SSH.`);
  }
  success('SSH reachable.');

  info('Checking /data mount...');
  const dataFs = requireRemoteOk(
    remoteTarget,
    "grep ' /data ' /proc/mounts | awk '{ print $3 }'",
    'Could not verify /data mount on remote device.',
  );
  if (dataFs !== 'ext4') {
    throw new Error(`/data must be mounted as ext4 before updating; got "${dataFs || 'not mounted'}".`);
  }
  success('/data is mounted as ext4.');

  info('Checking /boot mount...');
  const bootMount = requireRemoteOk(
    remoteTarget,
    "awk '$2 == \"/boot\" { print $3 \" \" $1; found=1 } END { if (!found) exit 1 }' /proc/mounts",
    'Could not verify /boot is mounted on the remote device.',
  );
  const [bootFs, bootSource] = bootMount.split(/\s+/, 2);
  if (bootFs !== 'vfat') {
    throw new Error(
      `/boot must be the mounted FAT boot partition before updating; got ${bootSource || 'unknown'} (${bootFs || 'unknown'}).`,
    );
  }

  const bootReady = requireRemoteOk(
    remoteTarget,
    'test -f /boot/cmdline.txt && test -f /boot/config.txt && echo ok',
    'Could not verify /boot has cmdline.txt and config.txt.',
  );
  if (bootReady !== 'ok') {
    throw new Error('/boot must be mounted with cmdline.txt and config.txt before updating.');
  }
  success(`/boot is mounted: ${bootSource} (${bootFs}).`);

  info('Preparing remote update directory...');
  const quotedDir = shellQuote(REMOTE_UPDATE_DIR);
  const updateDir = requireRemoteOk(
    remoteTarget,
    `mkdir -p ${quotedDir} && test -d ${quotedDir} && test -w ${quotedDir} && echo ok`,
    `Failed to prepare writable remote update directory ${REMOTE_UPDATE_DIR}.`,
  );
  if (updateDir !== 'ok') {
    throw new Error(`Remote update directory ${REMOTE_UPDATE_DIR} is not writable.`);
  }
  success(`Remote update directory ready: ${REMOTE_UPDATE_DIR}`);

  info('Checking A/B boot manager...');
  const slotStatus = requireRemoteOk(
    remoteTarget,
    'ab-boot-manager status',
    'Remote device does not appear to have ab-boot-manager available.',
  );
  log(slotStatus);
  success('A/B boot manager ready.');
}

function verifyRemoteSpace(remoteTarget, payloadSize) {
  info('Checking remote update space...');
  const remoteSpace = getRemoteTmpSpace(remoteTarget, REMOTE_UPDATE_DIR);
  const required = payloadSize + REMOTE_UPDATE_HEADROOM_BYTES;

  if (remoteSpace <= 0) {
    throw new Error(`Could not determine available space in ${REMOTE_UPDATE_DIR}.`);
  }

  if (remoteSpace < required) {
    throw new Error(
      `Not enough space in ${REMOTE_UPDATE_DIR}. ` +
        `Need ${formatBytes(required)} including headroom, have ${formatBytes(remoteSpace)}.`,
    );
  }

  success(`Remote has enough space (${formatBytes(remoteSpace)} available).`);
}

function removeStaleRemoteUpdateFiles(remoteTarget, paths) {
  if (paths.length === 0) {
    return;
  }

  info('Removing stale remote update artifacts...');
  const quotedPaths = paths.map(path => shellQuote(path)).join(' ');
  requireRemoteOk(
    remoteTarget,
    `rm -f ${quotedPaths}`,
    'Could not remove stale remote update artifacts.',
  );
  success('Stale remote update artifacts removed.');
}

async function ensureRemoteUpdateScript(remoteTarget) {
  const wrapper = ssh(remoteTarget, 'command -v mba-ab-update', { timeout: 10 });
  if (wrapper) {
    info(`Remote update helper: ${wrapper}`);
    return wrapper;
  }

  const existing = ssh(remoteTarget, 'command -v ab-update-with-key', { timeout: 10 });
  if (existing) {
    info(`Remote update helper: ${existing} (legacy bootstrap path)`);
    return 'ab-update-with-key';
  }

  if (!existsSync(LOCAL_UPDATE_SCRIPT)) {
    throw new Error(`Local update helper not found: ${LOCAL_UPDATE_SCRIPT}`);
  }

  info('ab-update-with-key not found on target; transferring bootstrap helper...');
  const remoteScriptPath = `${REMOTE_UPDATE_DIR}/ab-update-with-key`;
  await run('scp', [
    '-o',
    'ConnectTimeout=10',
    '-o',
    'BatchMode=yes',
    LOCAL_UPDATE_SCRIPT,
    `${remoteTarget}:${remoteScriptPath}`,
  ]);
  await run('ssh', [
    '-o',
    'ConnectTimeout=10',
    '-o',
    'BatchMode=yes',
    remoteTarget,
    `chmod 755 ${shellQuote(remoteScriptPath)}`,
  ]);
  success('Remote update helper transferred.');
  return remoteScriptPath;
}

async function runSmokeTest(host, user) {
  info('Running remote smoke test...');
  await run('npm', ['run', 'smoke', '--', '--host', host, '--user', user], { cwd: YOCTO_DIR });
}

async function main() {
  const args = process.argv.slice(2);

  if (args.includes('-h') || args.includes('--help')) {
    showHelp();
    return;
  }

  const host = argValue(args, ['--host', '--target'], DEFAULT_HOST);
  const user = argValue(args, '--user', DEFAULT_USER);
  const remoteTarget = `${user}@${host}`;
  const skipBuild = args.includes('--skip-build');
  const dryRun = args.includes('--dry-run');
  const skipConfirm = args.includes('--yes');
  const noWait = args.includes('--no-wait');
  const runSmoke = args.includes('--smoke');

  log('');
  log(`${colors.bold}${colors.cyan}Matchbox Audio Remote Update${colors.reset}`);
  if (dryRun) {
    log(`${colors.yellow}(dry-run mode - no remote changes will be made)${colors.reset}`);
  }
  log('');
  info(`Target: ${remoteTarget}`);
  log('');

  const config = await ensureSshKeyConfig();

  if (!dryRun) {
    preflightRemote(remoteTarget);
    log('');
  }

  if (!skipBuild) {
    info('Building remote update payload...');
    await run('npm', ['run', 'build', '--', '--update-payload'], { cwd: YOCTO_DIR });
  }

  const image = findUpdateImage();
  if (!image) {
    throw new Error('No image found. Run "npm run build" first.');
  }

  info(`Image: ${image.name}`);
  info(`Image type: ${image.kind === 'rootfs' ? 'rootfs payload' : 'WIC image'}`);
  info(`Size: ${formatBytes(image.stat.size)}`);

  let prepared = null;
  try {
    if (dryRun) {
      if (image.kind === 'rootfs') {
        info('Would transfer standalone rootfs artifact.');
      } else {
        info('Would prepare rootfs image from WIC.');
      }
      info(`Would create ${REMOTE_UPDATE_DIR}, check /data and free space, transfer payload, flash inactive slot, and reboot.`);
      return;
    }

    prepared = await prepareUpdatePayload(image);
    const payloadSize = statSync(prepared.preparedRootfsPath).size;
    verifyRemoteSpace(remoteTarget, payloadSize);
    ensureRemoteBootConfig(remoteTarget);

    const checksum = await calculateChecksum(prepared.preparedRootfsPath);
    const remoteImagePath = `${REMOTE_UPDATE_DIR}/${basename(prepared.preparedRootfsPath)}`;
    const remoteChecksumPath = `${remoteImagePath}.sha256`;
    const remoteKeyPath = `${REMOTE_UPDATE_DIR}/${basename(config.ssh_key_path)}`;
    removeStaleRemoteUpdateFiles(remoteTarget, [
      remoteImagePath,
      remoteChecksumPath,
      remoteKeyPath,
    ]);

    const transfer = await transferImage(
      prepared.preparedRootfsPath,
      checksum,
      remoteTarget,
      REMOTE_UPDATE_DIR,
    );

    if (!verifyRemoteChecksum(transfer.remoteImagePath, transfer.remoteChecksumPath, remoteTarget)) {
      throw new Error('Remote checksum verification failed.');
    }

    await run('scp', [
      '-o',
      'ConnectTimeout=10',
      '-o',
      'BatchMode=yes',
      config.ssh_key_path,
      `${remoteTarget}:${remoteKeyPath}`,
    ]);

    const remoteUpdateScript = await ensureRemoteUpdateScript(remoteTarget);
    const sudoRemoteUpdateScript = `sudo -n ${shellQuote(remoteUpdateScript)}`;

    await remoteFlashWithKey(
      transfer.remoteImagePath,
      remoteKeyPath,
      SSH_USERNAME,
      remoteTarget,
      false,
      skipConfirm,
      sudoRemoteUpdateScript,
    );

    if (!noWait) {
      const rebooted = await waitForReboot(remoteTarget, host, 0, 180);
      if (!rebooted) {
        throw new Error(`Timed out waiting for ${host} to reboot.`);
      }
      runCapture(
        `ssh -o ConnectTimeout=10 -o BatchMode=yes ${remoteTarget} "rm -f ${transfer.remoteImagePath} ${transfer.remoteChecksumPath} ${remoteKeyPath}"`,
      );
      if (runSmoke) {
        await runSmokeTest(host, user);
      }
    } else if (runSmoke) {
      warn('--smoke requested, but --no-wait prevents running the smoke test.');
    }

    success('Remote update complete.');
  } finally {
    if (prepared?.workDir) {
      rmSync(prepared.workDir, { recursive: true, force: true });
    }
  }
}

main().catch(err => {
  error(err.message);
  process.exit(1);
});
