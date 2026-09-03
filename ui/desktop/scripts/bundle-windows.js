#!/usr/bin/env node
/**
 * Package Goose Desktop for Windows from any host (macOS, Linux, or Windows).
 *
 * Electron's win32 runtime can be downloaded and packaged cross-platform.
 * The bundled backend must still be a real Windows goose.exe:
 *   1. GOOSE_WINDOWS_BINARY
 *   2. ui/desktop/src/bin/goose.exe
 *   3. target/x86_64-pc-windows-msvc/release/goose.exe
 *   4. Download goose-x86_64-pc-windows-msvc.zip from GitHub (default: matching app version)
 *
 * Usage (from ui/desktop):
 *   GITHUB_OWNER=vincenzopalazzo GITHUB_REPO=goose pnpm run bundle:windows
 *
 * Output:
 *   out/Goose-win32-x64/Goose.exe
 *   out/Goose-win32-x64.zip
 */

const fs = require('fs');
const os = require('os');
const path = require('path');
const https = require('https');
const { spawnSync } = require('child_process');

const desktopRoot = path.resolve(__dirname, '..');
const repoRoot = path.resolve(desktopRoot, '..', '..');
const srcBinDir = path.join(desktopRoot, 'src', 'bin');
const packageJson = JSON.parse(fs.readFileSync(path.join(desktopRoot, 'package.json'), 'utf8'));

const bundleName = process.env.GOOSE_BUNDLE_NAME || packageJson.productName || 'Goose';
const appVersion = process.env.GOOSE_VERSION || packageJson.version;
const githubOwner = process.env.GITHUB_OWNER || 'aaif-goose';
const githubRepo = process.env.GITHUB_REPO || 'goose';
const cliOwner = process.env.GOOSE_WINDOWS_CLI_OWNER || 'aaif-goose';
const cliRepo = process.env.GOOSE_WINDOWS_CLI_REPO || 'goose';
const cliTag = process.env.GOOSE_WINDOWS_CLI_TAG || `v${appVersion}`;
const cliAsset = process.env.GOOSE_WINDOWS_CLI_ASSET || 'goose-x86_64-pc-windows-msvc.zip';
const cliUrl =
  process.env.GOOSE_WINDOWS_CLI_URL ||
  `https://github.com/${cliOwner}/${cliRepo}/releases/download/${cliTag}/${cliAsset}`;

function log(message) {
  console.log(`[bundle-windows] ${message}`);
}

function fail(message) {
  console.error(`[bundle-windows] ${message}`);
  process.exit(1);
}

function run(command, args, options = {}) {
  log(`$ ${command} ${args.join(' ')}`);
  const result = spawnSync(command, args, {
    stdio: 'inherit',
    cwd: options.cwd || desktopRoot,
    env: { ...process.env, ...options.env },
    shell: process.platform === 'win32',
  });
  if (result.status !== 0) {
    fail(`${command} failed with exit code ${result.status}`);
  }
}

function downloadFile(url, destPath, redirectsRemaining = 5) {
  return new Promise((resolve, reject) => {
    https
      .get(url, { headers: { 'User-Agent': 'goose-bundle-windows' } }, (response) => {
        if (
          response.statusCode >= 300 &&
          response.statusCode < 400 &&
          response.headers.location &&
          redirectsRemaining > 0
        ) {
          response.resume();
          downloadFile(response.headers.location, destPath, redirectsRemaining - 1)
            .then(resolve)
            .catch(reject);
          return;
        }

        if (response.statusCode !== 200) {
          response.resume();
          reject(new Error(`Failed to download ${url}: HTTP ${response.statusCode}`));
          return;
        }

        const file = fs.createWriteStream(destPath);
        response.pipe(file);
        file.on('finish', () => file.close(resolve));
        file.on('error', reject);
      })
      .on('error', reject);
  });
}

function extractZip(zipPath, destDir) {
  fs.mkdirSync(destDir, { recursive: true });
  if (process.platform === 'win32') {
    run('powershell.exe', [
      '-NoProfile',
      '-ExecutionPolicy',
      'Bypass',
      '-Command',
      `Expand-Archive -LiteralPath '${zipPath.replace(/'/g, "''")}' -DestinationPath '${destDir.replace(/'/g, "''")}' -Force`,
    ]);
    return;
  }
  run('unzip', ['-q', '-o', zipPath, '-d', destDir], { cwd: destDir });
}

function findFile(dir, name) {
  if (!fs.existsSync(dir)) {
    return null;
  }
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isFile() && entry.name.toLowerCase() === name.toLowerCase()) {
      return full;
    }
    if (entry.isDirectory()) {
      const nested = findFile(full, name);
      if (nested) {
        return nested;
      }
    }
  }
  return null;
}

function unlinkIfExists(filePath) {
  fs.rmSync(filePath, { force: true });
}

async function resolveGooseExe() {
  const candidates = [
    process.env.GOOSE_WINDOWS_BINARY,
    path.join(srcBinDir, 'goose.exe'),
    path.join(repoRoot, 'target', 'x86_64-pc-windows-msvc', 'release', 'goose.exe'),
  ].filter(Boolean);

  for (const candidate of candidates) {
    if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) {
      log(`Using existing Windows backend: ${candidate}`);
      return candidate;
    }
  }

  log(`Windows goose.exe not found locally; downloading ${cliUrl}`);
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'goose-win-cli-'));
  const zipPath = path.join(tmpDir, 'goose-windows-cli.zip');
  const extractDir = path.join(tmpDir, 'extract');
  try {
    await downloadFile(cliUrl, zipPath);
    extractZip(zipPath, extractDir);
    const gooseExe = findFile(extractDir, 'goose.exe');
    if (!gooseExe) {
      throw new Error(`Downloaded ${cliAsset} did not contain goose.exe`);
    }
    const dest = path.join(tmpDir, 'goose.exe');
    fs.copyFileSync(gooseExe, dest);
    return dest;
  } catch (error) {
    fail(
      `Could not obtain goose.exe. Set GOOSE_WINDOWS_BINARY to a Windows goose.exe, or GOOSE_WINDOWS_CLI_URL to a zip that contains it. ${error.message}`
    );
  }
}

function snapshotHostBinaries() {
  if (process.platform === 'win32' || !fs.existsSync(srcBinDir)) {
    return null;
  }

  const snapshotDir = fs.mkdtempSync(path.join(os.tmpdir(), 'goose-host-bin-'));
  for (const entry of fs.readdirSync(srcBinDir, { withFileTypes: true })) {
    if (entry.name === 'goose.exe' || entry.name.endsWith('.dll') || entry.name.endsWith('.cmd')) {
      continue;
    }
    fs.cpSync(path.join(srcBinDir, entry.name), path.join(snapshotDir, entry.name), {
      recursive: true,
    });
  }
  log(`Snapshotted host src/bin files to ${snapshotDir}`);
  return snapshotDir;
}

function isWindowsRuntimeFile(name) {
  return (
    name === 'goose.exe' ||
    name === 'uv.exe' ||
    name === 'uvx.exe' ||
    name.endsWith('.dll') ||
    name.endsWith('.cmd')
  );
}

function restoreHostBinaries(snapshotDir) {
  if (!snapshotDir || !fs.existsSync(snapshotDir)) {
    return;
  }

  fs.mkdirSync(srcBinDir, { recursive: true });
  for (const entry of fs.readdirSync(srcBinDir, { withFileTypes: true })) {
    fs.rmSync(path.join(srcBinDir, entry.name), { recursive: true, force: true });
  }
  for (const entry of fs.readdirSync(snapshotDir, { withFileTypes: true })) {
    if (isWindowsRuntimeFile(entry.name)) {
      continue;
    }
    fs.cpSync(path.join(snapshotDir, entry.name), path.join(srcBinDir, entry.name), {
      recursive: true,
    });
  }
  fs.rmSync(snapshotDir, { recursive: true, force: true });
  log('Restored host src/bin files after Windows packaging');
}

function copyGooseExe(sourcePath) {
  fs.mkdirSync(srcBinDir, { recursive: true });
  const dest = path.join(srcBinDir, 'goose.exe');
  if (path.resolve(sourcePath) === dest) {
    return dest;
  }
  unlinkIfExists(dest);
  fs.copyFileSync(sourcePath, dest);
  log(`Copied goose.exe -> ${dest}`);
  return dest;
}

function writeAppUpdateYml(appDir) {
  const contents = [
    'provider: github',
    `owner: ${githubOwner}`,
    `repo: ${githubRepo}`,
    'updaterCacheDirName: goose-updater',
    '',
  ].join('\n');

  const resourcePath = path.join(appDir, 'resources', 'app-update.yml');
  fs.mkdirSync(path.dirname(resourcePath), { recursive: true });
  fs.writeFileSync(resourcePath, contents);
  log(`Wrote ${resourcePath} -> ${githubOwner}/${githubRepo}`);
}

function copyBinIntoResources(appDir) {
  const destBin = path.join(appDir, 'resources', 'bin');
  fs.mkdirSync(destBin, { recursive: true });
  for (const entry of fs.readdirSync(srcBinDir, { withFileTypes: true })) {
    const from = path.join(srcBinDir, entry.name);
    const to = path.join(destBin, entry.name);
    fs.rmSync(to, { recursive: true, force: true });
    if (entry.isDirectory()) {
      fs.cpSync(from, to, { recursive: true });
    } else {
      fs.copyFileSync(from, to);
    }
  }
  const gooseExe = path.join(destBin, 'goose.exe');
  if (!fs.existsSync(gooseExe)) {
    fail(`Packaged app is missing ${gooseExe}`);
  }
  log(`Copied Windows runtime files into ${destBin}`);
}

function zipAppDir(appDir, zipPath) {
  unlinkIfExists(zipPath);
  const parent = path.dirname(appDir);
  const name = path.basename(appDir);

  if (process.platform === 'darwin') {
    run('ditto', ['-c', '-k', '--sequesterRsrc', '--keepParent', name, zipPath], { cwd: parent });
    return;
  }

  if (process.platform === 'win32') {
    run('powershell.exe', [
      '-NoProfile',
      '-ExecutionPolicy',
      'Bypass',
      '-Command',
      `Compress-Archive -Path '${appDir.replace(/'/g, "''")}\\*' -DestinationPath '${zipPath.replace(/'/g, "''")}' -Force`,
    ]);
    return;
  }

  run('zip', ['-r', '-q', zipPath, name], { cwd: parent });
}

async function main() {
  log(`Packaging ${bundleName} ${appVersion} for win32-x64 from ${process.platform}`);
  log(`Update feed: ${githubOwner}/${githubRepo}`);

  const hostBinSnapshot = snapshotHostBinaries();
  try {
    const gooseExe = await resolveGooseExe();
    copyGooseExe(gooseExe);

    process.env.ELECTRON_PLATFORM = 'win32';
    process.env.ELECTRON_ARCH = 'x64';
    process.env.GITHUB_OWNER = githubOwner;
    process.env.GITHUB_REPO = githubRepo;
    process.env.GOOSE_BUNDLE_NAME = bundleName;

    run(process.execPath, [path.join(__dirname, 'prepare-platform-binaries.js')], {
      env: {
        ELECTRON_PLATFORM: 'win32',
        ELECTRON_ARCH: 'x64',
      },
    });

    if (!fs.existsSync(path.join(srcBinDir, 'goose.exe'))) {
      fail('src/bin/goose.exe missing after platform binary preparation');
    }

    run('pnpm', [
      'exec',
      'electron-forge',
      'package',
      '--platform=win32',
      '--arch=x64',
    ], {
      env: {
        ELECTRON_PLATFORM: 'win32',
        ELECTRON_ARCH: 'x64',
        GITHUB_OWNER: githubOwner,
        GITHUB_REPO: githubRepo,
        GOOSE_BUNDLE_NAME: bundleName,
      },
    });

    const appDir = path.join(desktopRoot, 'out', `${bundleName}-win32-x64`);
    const exePath = path.join(appDir, `${bundleName}.exe`);
    if (!fs.existsSync(exePath)) {
      fail(`Expected packaged executable not found: ${exePath}`);
    }

    copyBinIntoResources(appDir);
    writeAppUpdateYml(appDir);

    const zipPath = path.join(desktopRoot, 'out', `${bundleName}-win32-x64.zip`);
    zipAppDir(appDir, zipPath);

    const zipStat = fs.statSync(zipPath);
    log('Windows package complete');
    log(`  App: ${exePath}`);
    log(`  Zip: ${zipPath} (${Math.round(zipStat.size / 1024 / 1024)} MB)`);
  } finally {
    restoreHostBinaries(hostBinSnapshot);
  }
}

main().catch((error) => {
  fail(error.stack || error.message);
});
