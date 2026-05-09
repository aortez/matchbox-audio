#!/usr/bin/env node

import { spawnSync } from 'child_process';
import { existsSync, mkdirSync, readdirSync, rmSync, symlinkSync } from 'fs';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

import {
  colors,
  error,
  info,
  log,
  run,
  success,
} from '../pi-base/scripts/lib/index.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
const YOCTO_DIR = dirname(__dirname);
const KAS_FILE = 'kas-matchbox-audio-zero2.yml';
const IMAGE_NAME = 'matchbox-audio-image';
const GNU_HOSTTOOLS_DIR = join(YOCTO_DIR, '.gnu-hosttools');
const BITBAKE_HOSTTOOLS_DIR = join(YOCTO_DIR, 'build/tmp/hosttools');

function showHelp() {
  log(`
Matchbox Audio Yocto Build

Usage:
  npm run build [options]

Options:
  --clean      Clean image sstate before building
  -h, --help   Show this help
`);
}

function commandVersion(command, env) {
  const result = spawnSync(command, ['--version'], {
    encoding: 'utf8',
    env,
  });
  return `${result.stdout || ''}${result.stderr || ''}`.trim();
}

function createGnuHosttoolAliases() {
  if (!existsSync('/usr/bin/gnuinstall')) {
    return null;
  }

  rmSync(GNU_HOSTTOOLS_DIR, { recursive: true, force: true });
  mkdirSync(GNU_HOSTTOOLS_DIR, { recursive: true });

  for (const dir of ['/usr/bin', '/usr/sbin']) {
    if (!existsSync(dir)) {
      continue;
    }

    for (const entry of readdirSync(dir)) {
      if (!entry.startsWith('gnu') || entry.length <= 3) {
        continue;
      }

      symlinkSync(join(dir, entry), join(GNU_HOSTTOOLS_DIR, entry.slice(3)));
    }
  }

  return GNU_HOSTTOOLS_DIR;
}

function prepareBuildEnv() {
  const env = { ...process.env };
  const hosttoolsPath = createGnuHosttoolAliases();

  if (hosttoolsPath) {
    env.PATH = `${hosttoolsPath}:${env.PATH}`;
    rmSync(BITBAKE_HOSTTOOLS_DIR, { recursive: true, force: true });
    info(`Using GNU coreutils host tools from ${hosttoolsPath}`);
  }

  const installVersion = commandVersion('install', env);
  if (!installVersion.includes('GNU coreutils')) {
    const versionLine = installVersion.split('\n')[0] || 'unknown install implementation';
    throw new Error(
      `Yocto requires GNU coreutils install; current install reports "${versionLine}". ` +
        'Install gnu-coreutils or coreutils-from-gnu, then rerun npm run build.',
    );
  }

  return { env, hosttoolsPath };
}

function shellQuote(value) {
  return `'${value.replaceAll("'", "'\\''")}'`;
}

function bitbakeCommand(command, hosttoolsPath) {
  if (!hosttoolsPath) {
    return command;
  }

  return `export PATH=${shellQuote(hosttoolsPath)}:$PATH; ${command}`;
}

async function main() {
  const args = process.argv.slice(2);

  if (args.includes('-h') || args.includes('--help')) {
    showHelp();
    return;
  }

  log('');
  log(`${colors.bold}${colors.cyan}Matchbox Audio Yocto Build${colors.reset}`);
  log('');
  info('Target: Raspberry Pi Zero 2 W');
  info(`KAS config: ${KAS_FILE}`);
  log('');

  const { env, hosttoolsPath } = prepareBuildEnv();

  if (args.includes('--clean')) {
    info(`Cleaning ${IMAGE_NAME} sstate...`);
    await run('kas', ['shell', KAS_FILE, '-c', bitbakeCommand(`bitbake -c cleansstate ${IMAGE_NAME}`, hosttoolsPath)], {
      cwd: YOCTO_DIR,
      env,
    });
    success('Clean complete.');
  }

  info('Starting build...');
  await run('kas', ['shell', KAS_FILE, '-c', bitbakeCommand(`bitbake -c build ${IMAGE_NAME}`, hosttoolsPath)], {
    cwd: YOCTO_DIR,
    env,
  });

  log('');
  success('Build complete.');
  info('Image directory: build/tmp/deploy/images/raspberrypi0-2w/');
}

main().catch(err => {
  error(err.message);
  process.exit(1);
});
