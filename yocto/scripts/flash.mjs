#!/usr/bin/env node

import { execFileSync } from 'child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'fs';
import { tmpdir } from 'os';
import { basename, dirname, join } from 'path';
import { fileURLToPath } from 'url';

import {
  backupDataPartition,
  cleanupBackup,
  colors,
  configureSSHKey,
  error,
  findLatestImage,
  flashImage,
  formatBytes,
  getBlockDevices,
  getPartitionDevice,
  getWifiCredentials,
  growDataPartition,
  hasDataPartition,
  info,
  injectSSHKey,
  injectWifiCredentials,
  loadConfig,
  log,
  prompt,
  restoreDataPartition,
  saveConfig,
  setHostname,
  success,
  warn,
} from '../pi-base/scripts/lib/index.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
const YOCTO_DIR = dirname(__dirname);
const REPO_ROOT = dirname(YOCTO_DIR);
const CONFIG_FILE = join(YOCTO_DIR, '.flash-config.json');
const WIFI_CREDS_FILE = join(YOCTO_DIR, 'wifi-creds.local');
const HOTSPOT_CONFIG_FILE = join(REPO_ROOT, 'config/hotspot.local.json');
const DEFAULT_HOSTNAME = 'matchbox-audio';
const SSH_USERNAME = 'matchbox';
const SSH_UID = 1000;
const DATA_FREE_PERCENT = 10;

const IMAGE_DIR = join(YOCTO_DIR, 'build/tmp/deploy/images/raspberrypi0-2w');
const PREFERRED_IMAGES = [
  'matchbox-audio-image-raspberrypi0-2w.rootfs.wic.gz',
  'matchbox-audio-image-raspberrypi0-2w.wic.gz',
];

async function ensureSshKeyConfig(forceReconfigure = false) {
  if (forceReconfigure) {
    return configureSSHKey(CONFIG_FILE);
  }

  const config = loadConfig(CONFIG_FILE);
  if (config && config.ssh_key_path) {
    info(`Using SSH key: ${basename(config.ssh_key_path)}`);
    return config;
  }

  info('No SSH key configured yet.');
  return configureSSHKey(CONFIG_FILE);
}

function showHelp() {
  log(`
Matchbox Audio Flash Tool

Usage:
  npm run flash [options]

Options:
  --device <dev>   Flash directly to a device, for example /dev/sdb
  --list           List candidate devices and exit
  --dry-run        Show planned work without writing
  --skip-wifi      Do not inject home Wi-Fi credentials
  --reconfigure    Re-select SSH key
  -h, --help       Show this help
`);
}

async function selectTargetDevice(devices, specifiedDevice) {
  if (specifiedDevice) {
    const found = devices.find(device => device.device === specifiedDevice);
    if (!found) {
      throw new Error(`Device ${specifiedDevice} was not found or is not suitable for flashing.`);
    }
    return specifiedDevice;
  }

  log(`${colors.bold}Available devices:${colors.reset}`);
  log('');
  devices.forEach((device, index) => {
    const removable = device.removable ? `${colors.green}[removable]${colors.reset}` : '';
    log(`  ${colors.cyan}${index + 1})${colors.reset} ${device.device}  ${device.size}  ${device.model}  ${removable}`);
  });
  log('');

  const choice = await prompt(`Select device (1-${devices.length}) or q to quit: `);
  if (choice.toLowerCase() === 'q') {
    info('Aborted.');
    process.exit(0);
  }

  const index = Number.parseInt(choice, 10) - 1;
  if (Number.isNaN(index) || index < 0 || index >= devices.length) {
    throw new Error('Invalid device selection.');
  }

  return devices[index].device;
}

function displayCandidateDevices(devices) {
  log(`${colors.bold}Available devices:${colors.reset}`);
  log('');
  devices.forEach((device, index) => {
    const removable = device.removable ? `${colors.green}[removable]${colors.reset}` : '';
    log(`  ${colors.cyan}${index + 1})${colors.reset} ${device.device}  ${device.size}  ${device.model}  ${removable}`);
  });
  log('');
}

async function chooseHostname(config, specifiedDevice, dryRun) {
  let hostname = config.hostname || DEFAULT_HOSTNAME;
  if (specifiedDevice || dryRun) {
    return hostname;
  }

  const input = await prompt(`Device hostname (default: ${hostname}): `);
  if (input.trim()) {
    const cleaned = input.trim();
    if (/^[a-zA-Z0-9][a-zA-Z0-9-]*$/.test(cleaned)) {
      hostname = cleaned;
    } else {
      warn(`Invalid hostname "${cleaned}", using ${hostname}.`);
    }
  }

  config.hostname = hostname;
  saveConfig(CONFIG_FILE, config);
  return hostname;
}

function loadHotspotConfig(configPath) {
  if (!existsSync(configPath)) {
    warn('No config/hotspot.local.json found; the Pi will generate default hotspot credentials.');
    return null;
  }

  let parsed;
  try {
    parsed = JSON.parse(readFileSync(configPath, 'utf8'));
  } catch (err) {
    throw new Error(`${configPath} is not valid JSON: ${err.message}`);
  }

  const hotspot = parsed.hotspot || {};
  const ssid = typeof hotspot.ssid === 'string' ? hotspot.ssid.trim() : '';
  const password = typeof hotspot.password === 'string' ? hotspot.password : '';

  if (!ssid) {
    throw new Error(`${configPath} must set hotspot.ssid.`);
  }
  if (password.length < 8) {
    throw new Error(`${configPath} hotspot.password must be at least 8 characters.`);
  }

  return { ssid, password };
}

function shellSingleQuote(value) {
  return `'${value.replaceAll("'", "'\\''")}'`;
}

function commandErrorDetail(err) {
  const stderr = err?.stderr?.toString().trim();
  if (stderr) {
    return stderr;
  }
  return err?.message || String(err);
}

function hotspotEnv(config) {
  return `HOTSPOT_SSID=${shellSingleQuote(config.ssid)}\nHOTSPOT_PASSWORD=${shellSingleQuote(config.password)}\n`;
}

function injectHotspotConfig(device, config, dryRun = false) {
  if (!config) {
    return;
  }

  const dataPartition = getPartitionDevice(device, 4);
  info(`Injecting hotspot config for SSID "${config.ssid}"...`);

  if (dryRun) {
    log(`  Would mount ${dataPartition}`);
    log('  Would write /data/matchbox-audio/network/hotspot.env');
    log('  Would unmount');
    return;
  }

  const workDir = mkdtempSync(join(tmpdir(), 'matchbox-hotspot-'));
  const mountPoint = join(workDir, 'data');
  const envPath = join(workDir, 'hotspot.env');
  let mounted = false;
  let pendingError = null;

  try {
    mkdirSync(mountPoint);
    writeFileSync(envPath, hotspotEnv(config), { mode: 0o600 });
    execFileSync('sudo', ['mount', dataPartition, mountPoint], { stdio: 'pipe' });
    mounted = true;
    execFileSync('sudo', ['mkdir', '-p', join(mountPoint, 'matchbox-audio/network')], { stdio: 'pipe' });
    execFileSync('sudo', ['install', '-m', '0600', '-o', 'root', '-g', 'root', envPath, join(mountPoint, 'matchbox-audio/network/hotspot.env')], { stdio: 'pipe' });
    success('Hotspot config injected.');
  } catch (err) {
    pendingError = err;
  } finally {
    if (mounted) {
      try {
        execFileSync('sudo', ['umount', mountPoint], { stdio: 'pipe' });
        mounted = false;
      } catch (err) {
        const message = `Failed to unmount ${mountPoint} after hotspot config injection: ${commandErrorDetail(err)}. Leaving ${workDir} in place to avoid deleting mounted data.`;
        warn(message);
        pendingError = pendingError
          ? new Error(`${commandErrorDetail(pendingError)}; ${message}`)
          : new Error(message);
      }
    }

    if (mounted) {
      rmSync(envPath, { force: true });
    } else {
      rmSync(workDir, { recursive: true, force: true });
    }
  }

  if (pendingError) {
    throw pendingError;
  }
}

async function main() {
  const args = process.argv.slice(2);

  if (args.includes('-h') || args.includes('--help')) {
    showHelp();
    return;
  }

  const dryRun = args.includes('--dry-run');
  const listOnly = args.includes('--list');
  const skipWifi = args.includes('--skip-wifi');
  const reconfigure = args.includes('--reconfigure');
  const deviceIndex = args.indexOf('--device');
  const specifiedDevice = deviceIndex === -1 ? null : args[deviceIndex + 1];
  if (deviceIndex !== -1 && !specifiedDevice) {
    throw new Error('--device requires a device path.');
  }

  log('');
  log(`${colors.bold}${colors.cyan}Matchbox Audio Flash Tool${colors.reset}`);
  if (dryRun) {
    log(`${colors.yellow}(dry-run mode - no writes will be made)${colors.reset}`);
  }
  log('');

  const devices = getBlockDevices();
  if (devices.length === 0) {
    throw new Error('No suitable SD card or USB storage devices found.');
  }
  if (listOnly) {
    displayCandidateDevices(devices);
    return;
  }

  const config = await ensureSshKeyConfig(reconfigure);

  const image = findLatestImage(IMAGE_DIR, '.wic.gz', PREFERRED_IMAGES);
  if (!image) {
    throw new Error('No image found. Run "npm run build" first.');
  }

  info(`Image: ${image.name}`);
  info(`Size: ${formatBytes(image.stat.size)}`);
  info(`Built: ${image.stat.mtime.toLocaleString()}`);

  const bmapPath = image.path.replace('.wic.gz', '.wic.bmap');
  if (existsSync(bmapPath)) {
    info('Bmap: available');
  }
  log('');

  const targetDevice = await selectTargetDevice(devices, specifiedDevice);
  const hostname = await chooseHostname(config, specifiedDevice, dryRun);
  const hotspotConfig = loadHotspotConfig(HOTSPOT_CONFIG_FILE);

  let backupDir = null;
  if (!dryRun && hasDataPartition(targetDevice)) {
    log('');
    info(`Found existing data partition on ${getPartitionDevice(targetDevice, 4)}.`);
    const doBackup = await prompt('Backup /data before flashing? (Y/n): ');
    if (doBackup.toLowerCase() !== 'n') {
      backupDir = backupDataPartition(targetDevice);
      if (!backupDir) {
        const continueAnyway = await prompt('Continue without backup? (y/N): ');
        if (continueAnyway.toLowerCase() !== 'y') {
          info('Aborted.');
          return;
        }
      }
    }
  }

  let wifiCredentials = null;
  if (!dryRun && !backupDir && !skipWifi) {
    wifiCredentials = await getWifiCredentials(WIFI_CREDS_FILE);
  }

  try {
    await flashImage(image.path, targetDevice, {
      dryRun,
      bmapPath: existsSync(bmapPath) ? bmapPath : null,
    });

    growDataPartition(targetDevice, DATA_FREE_PERCENT, dryRun);
    await injectSSHKey(targetDevice, config.ssh_key_path, SSH_USERNAME, SSH_UID, dryRun);
    await setHostname(targetDevice, hostname, dryRun);

    if (wifiCredentials && !backupDir) {
      await injectWifiCredentials(targetDevice, wifiCredentials.ssid, wifiCredentials.password, dryRun);
    }

    if (backupDir) {
      restoreDataPartition(targetDevice, backupDir, dryRun);
      cleanupBackup(backupDir);
    }

    injectHotspotConfig(targetDevice, hotspotConfig, dryRun);

    log('');
    success(dryRun ? 'Dry run complete.' : 'Flash complete.');
    if (!dryRun) {
      info(`Login: ssh ${SSH_USERNAME}@${hostname}.local`);
      info(`Smoke test: npm run smoke -- --host ${hostname}.local`);
    }
  } catch (err) {
    cleanupBackup(backupDir);
    throw err;
  }
}

main().catch(err => {
  error(err.message);
  process.exit(1);
});
