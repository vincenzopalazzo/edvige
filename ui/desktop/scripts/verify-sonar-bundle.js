#!/usr/bin/env node

const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');

const appPath = process.argv[2];
if (!appPath) {
  console.error('Usage: verify-sonar-bundle.js <path-to-app>');
  process.exit(1);
}

const binaries = ['goose', 'goose-sonar-bridge'].map((name) =>
  path.join(appPath, 'Contents', 'Resources', 'bin', name)
);

for (const binary of binaries) {
  const stat = fs.statSync(binary);
  if (!stat.isFile()) {
    throw new Error(`Expected bundled binary at ${binary}`);
  }
  if ((stat.mode & 0o111) === 0) {
    throw new Error(`Bundled binary is not executable: ${binary}`);
  }
  if (process.platform === 'darwin' && process.env.APPLE_TEAM_ID) {
    execFileSync('codesign', ['--verify', '--strict', '--verbose=2', binary], {
      stdio: 'inherit',
    });
  }
}

console.log(`Verified Sonar bridge bundle layout in ${appPath}`);
