#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import {
  existsSync,
  mkdirSync,
  writeFileSync,
} from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const DEFAULT_HOST = 'matchbox-audio.local';
const DEFAULT_USER = 'matchbox';
const DEFAULT_APP_ID = 'dev.matchbox.audio';
const DEFAULT_ACTIVITY = `${DEFAULT_APP_ID}/.MainActivity`;
const DEFAULT_OUTPUT = '/tmp/matchbox-android-real-ble-smoke.png';
const DEFAULT_TIMEOUT_MS = 30_000;
const DEFAULT_LOCK_DURATION_MS = 15_000;
const DEFAULT_ANDROID_STUDIO_JBR = '/home/oldman/.progs/android-studio/jbr';
const SMOKE_APP_ID = 'dev.matchbox.ble_smoke';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, '..');

const colors = {
  reset: '\x1b[0m',
  bold: '\x1b[1m',
  cyan: '\x1b[36m',
  green: '\x1b[32m',
  blue: '\x1b[34m',
  red: '\x1b[31m',
};

function log(message = '') {
  console.log(message);
}

function info(message) {
  log(`${colors.blue}i${colors.reset} ${message}`);
}

function success(message) {
  log(`${colors.green}OK${colors.reset} ${message}`);
}

function fail(message) {
  console.error(`${colors.red}ERR${colors.reset} ${message}`);
}

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

function flag(args, name) {
  return args.includes(name);
}

function showHelp() {
  log(`
Matchbox Android Real BLE Smoke

Usage:
  tools/android-real-ble-smoke.mjs [options]

Options:
  --host <host>        Pi hostname or IP address (default: ${DEFAULT_HOST})
  --target <host>      Alias for --host
  --user <user>        SSH user (default: ${DEFAULT_USER})
  --serial <serial>    ADB device serial. Required if multiple devices are attached.
  --app-id <id>        Android app id (default: ${DEFAULT_APP_ID})
  --activity <name>    Activity component (default: ${DEFAULT_ACTIVITY})
  --app-dir <path>     Android Gradle project dir (default: android/)
  --java-home <path>   JDK for Gradle installDebug
  --skip-install       Do not run ./gradlew installDebug first
  --no-restart-check   Skip force-stop/relaunch reconnect verification
  --no-lock-check      Skip screen lock/unlock verification
  --lock-duration <ms> How long to leave the phone asleep (default: ${DEFAULT_LOCK_DURATION_MS})
  --output <path>      Screenshot output path (default: ${DEFAULT_OUTPUT})
  --timeout <ms>       BLE/UI wait timeout (default: ${DEFAULT_TIMEOUT_MS})
  -h, --help           Show this help
`);
}

function commandText(result) {
  return [result.stdout, result.stderr]
    .filter(Boolean)
    .map(value => Buffer.isBuffer(value) ? value.toString('utf8') : value)
    .map(value => value.trim())
    .filter(Boolean)
    .join('\n');
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd || repoRoot,
    env: options.env || process.env,
    encoding: options.encoding ?? 'utf8',
    timeout: options.timeout ?? 20_000,
    maxBuffer: options.maxBuffer ?? 4 * 1024 * 1024,
  });

  if (result.error) {
    throw result.error;
  }
  if (!options.allowFailure && result.status !== 0) {
    throw new Error(commandText(result) || `${command} ${args.join(' ')} failed`);
  }

  return result;
}

function argProvided(args, names) {
  const aliases = Array.isArray(names) ? names : [names];
  return aliases.some(name => args.includes(name) || args.some(arg => arg.startsWith(`${name}=`)));
}

function javaCompiler(javaHome) {
  return javaHome ? resolve(javaHome, 'bin', 'javac') : '';
}

function hasJavaCompiler(javaHome) {
  return javaHome !== '' && existsSync(javaCompiler(javaHome));
}

function detectJavaHome(explicitJavaHome) {
  if (explicitJavaHome !== '') {
    if (!hasJavaCompiler(explicitJavaHome)) {
      throw new Error([
        `--java-home does not contain a Java compiler: ${explicitJavaHome}`,
        `Expected javac at: ${javaCompiler(explicitJavaHome)}`,
      ].join('\n'));
    }
    return explicitJavaHome;
  }

  const candidates = [
    process.env.JAVA_HOME || '',
    DEFAULT_ANDROID_STUDIO_JBR,
    '/opt/android-studio/jbr',
    '/usr/local/android-studio/jbr',
  ].filter((value, index, all) => value !== '' && all.indexOf(value) === index);

  const javaHome = candidates.find(hasJavaCompiler);
  if (javaHome) {
    return javaHome;
  }

  throw new Error([
    'No usable JDK found for Gradle installDebug.',
    'Set --java-home or JAVA_HOME to a JDK that contains bin/javac.',
    `Local default checked: ${DEFAULT_ANDROID_STUDIO_JBR}`,
  ].join('\n'));
}

function adb(serial, ...args) {
  const adbArgs = serial ? ['-s', serial, ...args] : args;
  return run('adb', adbArgs, { timeout: 30_000 }).stdout.trim();
}

function adbAllowFailure(serial, ...args) {
  const adbArgs = serial ? ['-s', serial, ...args] : args;
  return run('adb', adbArgs, { allowFailure: true, timeout: 30_000 });
}

function forceStopApp(serial, appId) {
  adbAllowFailure(serial, 'shell', 'am', 'force-stop', appId);
}

function wakePhone(serial) {
  adb(serial, 'shell', 'input', 'keyevent', 'KEYCODE_WAKEUP');
  adbAllowFailure(serial, 'shell', 'wm', 'dismiss-keyguard');
}

function sleepPhone(serial) {
  adb(serial, 'shell', 'input', 'keyevent', 'KEYCODE_SLEEP');
}

function ssh(remoteTarget, command) {
  return run('ssh', [
    '-o',
    'ConnectTimeout=5',
    '-o',
    'BatchMode=yes',
    remoteTarget,
    command,
  ], { timeout: 20_000 }).stdout.trim();
}

async function sleep(ms) {
  await new Promise(resolveSleep => setTimeout(resolveSleep, ms));
}

function ensureOneAdbDevice(serial) {
  const output = run('adb', ['devices'], { timeout: 10_000 }).stdout;
  const devices = output
    .split('\n')
    .slice(1)
    .map(line => line.trim())
    .filter(Boolean)
    .map(line => line.split(/\s+/))
    .filter(([, state]) => state === 'device')
    .map(([deviceSerial]) => deviceSerial);

  if (serial) {
    if (!devices.includes(serial)) {
      throw new Error(`ADB device ${serial} is not attached. Attached: ${devices.join(', ') || 'none'}`);
    }
    return serial;
  }

  if (devices.length !== 1) {
    throw new Error(`Expected exactly one attached ADB device, found ${devices.length}: ${devices.join(', ') || 'none'}`);
  }

  return devices[0];
}

function installDebugApp(appDir, javaHome) {
  const gradlew = resolve(appDir, 'gradlew');
  if (!existsSync(gradlew)) {
    throw new Error(`Gradle wrapper not found at ${gradlew}`);
  }

  const env = { ...process.env };
  env.JAVA_HOME = javaHome;

  run('./gradlew', ['installDebug'], {
    cwd: appDir,
    env,
    timeout: 180_000,
    maxBuffer: 8 * 1024 * 1024,
  });
}

async function piStatus(baseUrl) {
  const response = await fetch(`${baseUrl.replace(/\/$/, '')}/api/v1/status`);
  if (!response.ok) {
    throw new Error(`Pi status request failed: HTTP ${response.status}`);
  }
  return response.json();
}

function expectedUiValues(status) {
  const playback = status.playback;
  const track = playback?.track;

  if (!playback) {
    throw new Error('Pi status did not include playback');
  }

  const title = track ? (track.title || basename(track.uri)) : 'No track';
  const queue = queueLabel(playback.queue_position, playback.queue_length);

  return [
    'BLE ready',
    playback.state.toUpperCase(),
    title,
    track?.artist,
    track?.album,
    String(playback.volume),
    queue,
    status.network?.mode,
    status.network?.active_connection,
  ].filter(value => value !== undefined && value !== null && value !== '');
}

function basename(path) {
  const parts = String(path).split('/');
  return parts[parts.length - 1] || String(path);
}

function queueLabel(position, length) {
  if (!length || length <= 0) {
    return 'empty';
  }
  if (position === undefined || position === null) {
    return String(length);
  }
  return `${position + 1} / ${length}`;
}

function parseBtStatus(text) {
  const values = new Map();
  for (const line of text.split('\n')) {
    const match = line.match(/^([^:]+):\s*(.*)$/);
    if (match) {
      values.set(match[1], match[2]);
    }
  }
  return values;
}

async function waitForBtState(remoteTarget, timeoutMs, description, predicate) {
  const deadline = Date.now() + timeoutMs;
  let lastStatus = '';

  while (Date.now() < deadline) {
    lastStatus = ssh(remoteTarget, 'mba-cli bt status');
    const status = parseBtStatus(lastStatus);
    if (predicate(status)) {
      return lastStatus;
    }
    await sleep(1000);
  }

  throw new Error(`Timed out waiting for Pi BLE ${description}:\n${lastStatus}`);
}

async function waitForBtIdle(remoteTarget, timeoutMs) {
  return waitForBtState(
    remoteTarget,
    timeoutMs,
    'advertising idle state',
    status => status.get('advertising') === 'true' && status.get('busy') === 'false',
  );
}

async function waitForBtBusy(remoteTarget, timeoutMs) {
  return waitForBtState(
    remoteTarget,
    timeoutMs,
    'active session state',
    status => status.get('advertising') === 'true' && status.get('busy') === 'true',
  );
}

function dumpUi(serial) {
  adb(serial, 'shell', 'uiautomator', 'dump', '/sdcard/matchbox-window.xml');
  return adb(serial, 'shell', 'cat', '/sdcard/matchbox-window.xml');
}

function xmlText(value) {
  return value
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&#39;/g, "'")
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&amp;/g, '&');
}

function nodeAttributes(node) {
  const attributes = {};
  for (const match of node.matchAll(/([a-zA-Z0-9:-]+)="([^"]*)"/g)) {
    attributes[match[1]] = xmlText(match[2]);
  }
  return attributes;
}

function uiNodes(xml) {
  return [...xml.matchAll(/<node\b[^>]*>/g)].map(match => nodeAttributes(match[0]));
}

function visibleTexts(xml) {
  return uiNodes(xml)
    .map(node => node.text)
    .filter(Boolean);
}

function boundsCenter(bounds) {
  const match = bounds?.match(/^\[(\d+),(\d+)]\[(\d+),(\d+)]$/);
  if (!match) {
    return null;
  }
  const [, x1, y1, x2, y2] = match.map(Number);
  return {
    x: Math.floor((x1 + x2) / 2),
    y: Math.floor((y1 + y2) / 2),
  };
}

function findTextCenter(xml, text) {
  const node = uiNodes(xml).find(candidate => candidate.text === text);
  return boundsCenter(node?.bounds);
}

function tapText(serial, text) {
  const xml = dumpUi(serial);
  const point = findTextCenter(xml, text);
  if (!point) {
    throw new Error(`Could not find UI text to tap: ${text}`);
  }
  adb(serial, 'shell', 'input', 'tap', String(point.x), String(point.y));
}

function maybeTapText(serial, text) {
  const xml = dumpUi(serial);
  const point = findTextCenter(xml, text);
  if (!point) {
    return false;
  }
  adb(serial, 'shell', 'input', 'tap', String(point.x), String(point.y));
  return true;
}

async function waitForAnyAndroidText(serial, expected, timeoutMs) {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    const texts = visibleTexts(dumpUi(serial));
    const match = expected.find(value => texts.includes(value));
    if (match) {
      return match;
    }
    await sleep(250);
  }

  return null;
}

async function waitForAndroidValues(serial, expected, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let lastTexts = [];

  while (Date.now() < deadline) {
    const xml = dumpUi(serial);
    lastTexts = visibleTexts(xml);
    const missing = expected.filter(value => !lastTexts.includes(value));
    if (missing.length === 0) {
      return lastTexts;
    }
    await sleep(1000);
  }

  const missing = expected.filter(value => !lastTexts.includes(value));
  throw new Error([
    `Timed out waiting for Android UI to match Pi status.`,
    `Missing: ${missing.join(', ')}`,
    `Visible text: ${lastTexts.join(' | ')}`,
  ].join('\n'));
}

function captureScreenshot(serial, output) {
  const adbArgs = serial ? ['-s', serial, 'exec-out', 'screencap', '-p'] : ['exec-out', 'screencap', '-p'];
  const result = run('adb', adbArgs, {
    encoding: 'buffer',
    timeout: 20_000,
    maxBuffer: 20 * 1024 * 1024,
  });
  mkdirSync(dirname(output), { recursive: true });
  writeFileSync(output, result.stdout);
}

function knownDevicePrefs(serial, appId) {
  const result = adbAllowFailure(
    serial,
    'shell',
    'run-as',
    appId,
    'cat',
    'shared_prefs/matchbox_ble.xml',
  );
  const text = commandText(result);
  return { ok: result.status === 0, text };
}

async function waitForKnownDeviceStored(serial, appId, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let lastResult = { ok: false, text: '' };

  while (Date.now() < deadline) {
    lastResult = knownDevicePrefs(serial, appId);
    if (lastResult.ok && lastResult.text.includes('known_device_address')) {
      return;
    }
    await sleep(250);
  }

  if (!lastResult.ok) {
    throw new Error(`Could not read app BLE preferences with run-as:\n${lastResult.text}`);
  }
  throw new Error(`BLE known-device preference was not written:\n${lastResult.text}`);
}

async function launchConnectAndVerify(serial, activity, expected, timeoutMs) {
  adb(serial, 'shell', 'am', 'start', '-n', activity);
  await sleep(1000);
  maybeTapText(serial, 'Connect BLE');
  await sleep(250);
  maybeTapText(serial, 'Allow');
  maybeTapText(serial, 'While using the app');

  const phase = await waitForAnyAndroidText(
    serial,
    ['Reconnecting', 'Scanning', 'Connecting', 'BLE ready'],
    3000,
  );
  if (phase) {
    info(`Observed Android BLE phase: ${phase}`);
  }

  await waitForAndroidValues(serial, expected, timeoutMs);
}

async function expectedFromPi(baseUrl) {
  const status = await piStatus(baseUrl);
  return expectedUiValues(status);
}

async function main() {
  const args = process.argv.slice(2);
  if (flag(args, '-h') || flag(args, '--help')) {
    showHelp();
    return;
  }

  const host = argValue(args, ['--host', '--target'], DEFAULT_HOST);
  const user = argValue(args, '--user', DEFAULT_USER);
  const remoteTarget = `${user}@${host}`;
  const serial = argValue(args, '--serial', '');
  const appId = argValue(args, '--app-id', DEFAULT_APP_ID);
  const activity = argValue(args, '--activity', `${appId}/.MainActivity`);
  const appDir = resolve(repoRoot, argValue(args, '--app-dir', 'android'));
  const explicitJavaHome = argProvided(args, '--java-home') ? argValue(args, '--java-home', '') : '';
  const skipInstall = flag(args, '--skip-install');
  const restartCheck = !flag(args, '--no-restart-check');
  const lockCheck = !flag(args, '--no-lock-check');
  const lockDurationMs = Number(argValue(args, '--lock-duration', String(DEFAULT_LOCK_DURATION_MS)));
  const output = argValue(args, '--output', DEFAULT_OUTPUT);
  const timeoutMs = Number(argValue(args, '--timeout', String(DEFAULT_TIMEOUT_MS)));
  const baseUrl = `http://${host}:8090`;

  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    throw new Error('--timeout must be a positive number of milliseconds');
  }
  if (!Number.isFinite(lockDurationMs) || lockDurationMs <= 0) {
    throw new Error('--lock-duration must be a positive number of milliseconds');
  }

  log('');
  log(`${colors.bold}${colors.cyan}Matchbox Android Real BLE Smoke${colors.reset}`);
  log('');
  info(`Pi: ${remoteTarget}`);
  info(`Android app: ${appId}`);
  log('');

  const deviceSerial = ensureOneAdbDevice(serial);
  success(`ADB device attached: ${deviceSerial}`);

  if (!skipInstall) {
    const javaHome = detectJavaHome(explicitJavaHome);
    info(`Using JAVA_HOME: ${javaHome}`);
    info('Installing Android debug app');
    installDebugApp(appDir, javaHome);
    success('Android debug app installed');
  }

  info('Preparing phone and clearing previous BLE sessions');
  wakePhone(deviceSerial);
  adbAllowFailure(deviceSerial, 'shell', 'pm', 'grant', appId, 'android.permission.BLUETOOTH_SCAN');
  adbAllowFailure(deviceSerial, 'shell', 'pm', 'grant', appId, 'android.permission.BLUETOOTH_CONNECT');
  forceStopApp(deviceSerial, appId);
  forceStopApp(deviceSerial, SMOKE_APP_ID);
  await sleep(1500);
  success('Phone ready');

  info('Checking Pi BLE daemon');
  const btStatus = await waitForBtIdle(remoteTarget, timeoutMs);
  log(btStatus);
  success('Pi BLE is advertising and idle');

  info('Reading Pi playback status');
  const expected = await expectedFromPi(baseUrl);
  log(`Expecting: ${expected.join(' | ')}`);
  success('Pi status ready');

  info('Launching Android app and connecting BLE');
  await launchConnectAndVerify(deviceSerial, activity, expected, timeoutMs);
  success('Android BLE snapshot matches Pi status');
  await waitForBtBusy(remoteTarget, timeoutMs);
  success('Pi BLE reports the Android app as the active session');
  await waitForKnownDeviceStored(deviceSerial, appId, timeoutMs);
  success('Android app stored known BLE device');

  if (restartCheck) {
    info('Force-stopping Android app for reconnect check');
    forceStopApp(deviceSerial, appId);
    await sleep(1500);
    await waitForBtIdle(remoteTarget, timeoutMs);
    success('Pi BLE returned to idle after app stop');

    info('Relaunching Android app for known-device reconnect check');
    await launchConnectAndVerify(deviceSerial, activity, expected, timeoutMs);
    success('Android BLE reconnect after app restart matches Pi status');
    await waitForBtBusy(remoteTarget, timeoutMs);
    success('Pi BLE reports the reconnected app as the active session');
  }

  if (lockCheck) {
    info(`Sleeping phone for lock/unlock check (${lockDurationMs} ms)`);
    sleepPhone(deviceSerial);
    await sleep(lockDurationMs);

    info('Waking phone and returning app to foreground');
    wakePhone(deviceSerial);
    await sleep(1000);
    const lockExpected = await expectedFromPi(baseUrl);
    log(`Expecting after unlock: ${lockExpected.join(' | ')}`);
    await launchConnectAndVerify(deviceSerial, activity, lockExpected, timeoutMs);
    success('Android BLE snapshot matches Pi status after lock/unlock');
    await waitForBtBusy(remoteTarget, timeoutMs);
    success('Pi BLE still reports an active Android session after lock/unlock');
  }

  captureScreenshot(deviceSerial, output);
  success(`Screenshot captured: ${output}`);
}

main().catch(err => {
  fail(err.message);
  process.exit(1);
});
