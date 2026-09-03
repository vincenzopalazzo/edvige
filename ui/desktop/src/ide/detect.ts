import type { Dirent } from 'node:fs';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { IDE_CATALOG, type CatalogEntry } from './registry';
import type { DetectedIde, DetectionSource, IdeId } from '../types/ide';

const SOURCE_PRIORITY: Record<DetectionSource, number> = {
  path: 0,
  toolbox: 1,
  bundle: 2,
  'desktop-file': 3,
};

const BUNDLED_CLI_RELATIVES: Partial<Record<IdeId, string>> = {
  zed: 'Contents/MacOS/cli',
  vscode: 'Contents/Resources/app/bin/code',
  'vscode-insiders': 'Contents/Resources/app/bin/code-insiders',
  vscodium: 'Contents/Resources/app/bin/codium',
  cursor: 'Contents/Resources/app/bin/cursor',
};

const TOOLBOX_SCRIPT_IDS: Record<string, IdeId> = {
  'idea.sh': 'intellij-idea',
  'idea64.exe': 'intellij-idea',
  'pycharm.sh': 'pycharm',
  'pycharm64.exe': 'pycharm',
  'webstorm.sh': 'webstorm',
  'webstorm64.exe': 'webstorm',
  'goland.sh': 'goland',
  'goland64.exe': 'goland',
  'clion.sh': 'clion',
  'clion64.exe': 'clion',
  'rider.sh': 'rider',
  'rider64.exe': 'rider',
  'rubymine.sh': 'rubymine',
  'rubymine64.exe': 'rubymine',
  'phpstorm.sh': 'phpstorm',
  'phpstorm64.exe': 'phpstorm',
};

const WINDOWS_BUNDLE_LAYOUTS: Array<{ id: IdeId; dir: string; cliRelative: string }> = [
  { id: 'vscode', dir: 'Microsoft VS Code', cliRelative: 'bin/code.cmd' },
  {
    id: 'vscode-insiders',
    dir: 'Microsoft VS Code Insiders',
    cliRelative: 'bin/code-insiders.cmd',
  },
  { id: 'vscodium', dir: 'VSCodium', cliRelative: 'bin/codium.cmd' },
  { id: 'cursor', dir: 'Cursor', cliRelative: 'bin/cursor.cmd' },
  { id: 'windsurf', dir: 'Windsurf', cliRelative: 'bin/windsurf.cmd' },
  { id: 'sublime', dir: 'Sublime Text', cliRelative: 'subl.exe' },
];

const FIELD_CODE = /^%[a-zA-Z%]+$/;

export interface DesktopExec {
  path: string;
  args: string[];
}

export function parseDesktopExec(exec: string): DesktopExec {
  const tokens = tokenizeDesktopExec(exec).filter((token) => !FIELD_CODE.test(token));
  return { path: tokens[0] ?? '', args: tokens.slice(1) };
}

function tokenizeDesktopExec(exec: string): string[] {
  const tokens: string[] = [];
  let current = '';
  let quote: string | null = null;
  for (const char of exec.trim()) {
    if (quote) {
      if (char === quote) quote = null;
      else current += char;
    } else if (char === '"' || char === "'") {
      quote = char;
    } else if (/\s/.test(char)) {
      if (current.length > 0) tokens.push(current);
      current = '';
    } else {
      current += char;
    }
  }
  if (current.length > 0) tokens.push(current);
  return tokens;
}

export function toolboxScriptToId(scriptName: string): IdeId | null {
  return TOOLBOX_SCRIPT_IDS[scriptName.toLowerCase()] ?? null;
}

export function dedupeIdes(ides: DetectedIde[]): DetectedIde[] {
  const bestById = new Map<IdeId, DetectedIde>();
  for (const ide of ides) {
    const current = bestById.get(ide.id);
    if (!current || SOURCE_PRIORITY[ide.source] < SOURCE_PRIORITY[current.source]) {
      bestById.set(ide.id, ide);
    }
  }
  return [...bestById.values()].sort((a, b) =>
    IDE_CATALOG[a.id].name.localeCompare(IDE_CATALOG[b.id].name)
  );
}

const DETECT_CACHE_TTL_MS = 60_000;
let detectCache: { at: number; ides: DetectedIde[] } | null = null;

export async function detectIdes(): Promise<DetectedIde[]> {
  if (detectCache && Date.now() - detectCache.at < DETECT_CACHE_TTL_MS) {
    return detectCache.ides;
  }
  const layers = await Promise.all([
    detectFromPath(),
    detectMacBundles(),
    detectWindowsBundles(),
    detectWindowsJetbrains(),
    detectDesktopFiles(),
    detectMacToolbox(),
    detectToolboxScripts(),
  ]);
  const ides = dedupeIdes(layers.flat());
  detectCache = { at: Date.now(), ides };
  return ides;
}

function catalogEntries(): Array<[IdeId, CatalogEntry]> {
  return Object.entries(IDE_CATALOG) as Array<[IdeId, CatalogEntry]>;
}

async function isExecutable(filePath: string): Promise<boolean> {
  try {
    await fs.access(filePath, fs.constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

async function pathExists(filePath: string): Promise<boolean> {
  try {
    await fs.access(filePath);
    return true;
  } catch {
    return false;
  }
}

async function walkDirectories(root: string, maxDepth: number): Promise<string[]> {
  const directories: string[] = [];
  const visit = async (dir: string, depth: number): Promise<void> => {
    let entries: Dirent[];
    try {
      entries = await fs.readdir(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      const fullPath = path.join(dir, entry.name);
      directories.push(fullPath);
      if (entry.name.toLowerCase().endsWith('.app')) continue;
      if (depth < maxDepth) await visit(fullPath, depth + 1);
    }
  };
  await visit(root, 1);
  return directories;
}

async function detectFromPath(): Promise<DetectedIde[]> {
  const searchDirs = (process.env.PATH ?? '').split(path.delimiter).filter((dir) => dir.length > 0);
  const detected: DetectedIde[] = [];
  for (const [id, catalog] of catalogEntries()) {
    for (const cliName of catalog.cliNames) {
      const launcherPath = await findOnPath(searchDirs, cliName);
      if (launcherPath) {
        detected.push({ id, name: catalog.name, launcherPath, source: 'path' });
        break;
      }
    }
  }
  return detected;
}

async function findOnPath(searchDirs: string[], cliName: string): Promise<string | null> {
  const suffixes = process.platform === 'win32' ? ['', '.cmd', '.exe'] : [''];
  for (const dir of searchDirs) {
    for (const suffix of suffixes) {
      const candidate = path.join(dir, cliName + suffix);
      if (await isExecutable(candidate)) return candidate;
    }
  }
  return null;
}

async function detectMacBundles(): Promise<DetectedIde[]> {
  if (process.platform !== 'darwin') return [];
  const applicationDirs = ['/Applications', path.join(os.homedir(), 'Applications')];
  const seen = new Set<IdeId>();
  const detected: DetectedIde[] = [];
  for (const applicationDir of applicationDirs) {
    let installed: string[];
    try {
      installed = await fs.readdir(applicationDir);
    } catch {
      continue;
    }
    for (const [id, catalog] of catalogEntries()) {
      if (seen.has(id)) continue;
      const bundleName = installed.find((name) =>
        catalog.macApps.some((macApp) => macApp.toLowerCase() === name.toLowerCase())
      );
      if (!bundleName) continue;
      seen.add(id);
      detected.push(await macBundleIde(id, path.join(applicationDir, bundleName)));
    }
  }
  return detected;
}

async function macBundleIde(id: IdeId, bundlePath: string): Promise<DetectedIde> {
  const cliRelative = BUNDLED_CLI_RELATIVES[id];
  if (cliRelative) {
    const cliPath = path.join(bundlePath, ...cliRelative.split('/'));
    if (await isExecutable(cliPath)) {
      return { id, name: IDE_CATALOG[id].name, launcherPath: cliPath, source: 'bundle' };
    }
  }
  if (IDE_CATALOG[id].jetbrains) {
    const launcherPath = await macJetbrainsLauncher(bundlePath);
    if (launcherPath) {
      return { id, name: IDE_CATALOG[id].name, launcherPath, source: 'bundle' };
    }
  }
  return {
    id,
    name: IDE_CATALOG[id].name,
    launcherPath: '/usr/bin/open',
    bundlePath,
    source: 'bundle',
  };
}

async function detectWindowsJetbrains(): Promise<DetectedIde[]> {
  if (process.platform !== 'win32') return [];
  const roots = [
    process.env.ProgramFiles ?? 'C:\\Program Files',
    process.env['ProgramFiles(x86)'] ?? 'C:\\Program Files (x86)',
  ];
  const launcherById = new Map<IdeId, string>();
  for (const root of roots) {
    const jetbrainsRoot = path.join(root, 'JetBrains');
    let productDirs: string[];
    try {
      productDirs = await fs.readdir(jetbrainsRoot);
    } catch {
      continue;
    }
    const newestFirst = [...productDirs].sort((a, b) => b.localeCompare(a));
    for (const productDir of newestFirst) {
      const ide = await windowsJetbrainsIde(path.join(jetbrainsRoot, productDir));
      if (ide && !launcherById.has(ide.id)) launcherById.set(ide.id, ide.launcherPath);
    }
  }
  return [...launcherById.entries()].map(([id, launcherPath]) => ({
    id,
    name: IDE_CATALOG[id].name,
    launcherPath,
    source: 'bundle' as const,
  }));
}

async function windowsJetbrainsIde(productDir: string): Promise<DetectedIde | null> {
  const binDir = path.join(productDir, 'bin');
  let fileNames: string[];
  try {
    fileNames = await fs.readdir(binDir);
  } catch {
    return null;
  }
  for (const fileName of fileNames) {
    if (!fileName.toLowerCase().endsWith('.exe')) continue;
    const id = toolboxScriptToId(fileName);
    if (id) {
      return {
        id,
        name: IDE_CATALOG[id].name,
        launcherPath: path.join(binDir, fileName),
        source: 'bundle',
      };
    }
  }
  return null;
}

async function detectWindowsBundles(): Promise<DetectedIde[]> {
  if (process.platform !== 'win32') return [];
  const localAppData = process.env.LOCALAPPDATA ?? path.join(os.homedir(), 'AppData', 'Local');
  const roots = [
    path.join(localAppData, 'Programs'),
    process.env.ProgramFiles ?? 'C:\\Program Files',
    process.env['ProgramFiles(x86)'] ?? 'C:\\Program Files (x86)',
  ];
  const detected: DetectedIde[] = [];
  for (const layout of WINDOWS_BUNDLE_LAYOUTS) {
    for (const root of roots) {
      const cliPath = path.join(root, layout.dir, ...layout.cliRelative.split('/'));
      if (!(await pathExists(cliPath))) continue;
      detected.push({
        id: layout.id,
        name: IDE_CATALOG[layout.id].name,
        launcherPath: cliPath,
        source: 'bundle',
      });
      break;
    }
  }
  return detected;
}

interface DesktopEntry {
  name: string;
  exec: string;
  fileName?: string;
}

async function detectDesktopFiles(): Promise<DetectedIde[]> {
  if (process.platform !== 'linux') return [];
  const desktopDirs = [
    '/usr/share/applications',
    path.join(os.homedir(), '.local', 'share', 'applications'),
  ];
  const entries = await readDesktopEntries(desktopDirs);
  const detected: DetectedIde[] = [];
  for (const entry of entries) {
    const id = matchDesktopEntry(entry);
    const launcher = id ? parseDesktopExec(entry.exec) : null;
    if (id && launcher && launcher.path) {
      detected.push({
        id,
        name: IDE_CATALOG[id].name,
        launcherPath: launcher.path,
        launcherArgs: launcher.args.length > 0 ? launcher.args : undefined,
        source: 'desktop-file',
      });
    }
  }
  return detected;
}

async function readDesktopEntries(desktopDirs: string[]): Promise<DesktopEntry[]> {
  const entries: DesktopEntry[] = [];
  for (const dir of desktopDirs) {
    let fileNames: string[];
    try {
      fileNames = await fs.readdir(dir);
    } catch {
      continue;
    }
    for (const fileName of fileNames.filter((name) => name.endsWith('.desktop'))) {
      const entry = await readDesktopEntry(path.join(dir, fileName));
      if (entry) entries.push({ ...entry, fileName });
    }
  }
  return entries;
}

async function readDesktopEntry(filePath: string): Promise<DesktopEntry | null> {
  try {
    const contents = await fs.readFile(filePath, 'utf8');
    const name = /^Name=(.+)$/m.exec(contents)?.[1]?.trim();
    const exec = /^Exec=(.+)$/m.exec(contents)?.[1]?.trim();
    return name && exec ? { name, exec } : null;
  } catch {
    return null;
  }
}

function matchDesktopEntry(entry: DesktopEntry): IdeId | null {
  const fileStem = entry.fileName?.replace(/\.desktop$/i, '').toLowerCase() ?? '';
  const ordered = catalogEntries().sort(([, a], [, b]) => b.name.length - a.name.length);
  for (const [id, catalog] of ordered) {
    if (catalog.cliNames.some((cliName) => fileStem === cliName.toLowerCase())) return id;
  }
  const normalizedName = entry.name.toLowerCase();
  for (const [id, catalog] of ordered) {
    if (normalizedName.includes(catalog.name.toLowerCase())) return id;
  }
  return null;
}

async function detectMacToolbox(): Promise<DetectedIde[]> {
  if (process.platform !== 'darwin') return [];
  const appsRoot = toolboxAppsRoot();
  const directories = await walkDirectories(appsRoot, 3);
  const bundlePaths: string[] = [];
  for (const dir of directories) {
    if (dir.toLowerCase().endsWith('.app')) {
      bundlePaths.push(dir);
      continue;
    }
    for (const bundleName of await listBundleNames(dir)) {
      bundlePaths.push(path.join(dir, bundleName));
    }
  }
  const detected: DetectedIde[] = [];
  for (const bundlePath of bundlePaths) {
    const id = matchAppBundleId(path.basename(bundlePath));
    if (id) detected.push(await macToolboxIde(id, bundlePath));
  }
  return detected;
}

async function listBundleNames(dir: string): Promise<string[]> {
  try {
    return (await fs.readdir(dir)).filter((name) => name.toLowerCase().endsWith('.app'));
  } catch {
    return [];
  }
}

function matchAppBundleId(bundleName: string): IdeId | null {
  const normalized = bundleName.toLowerCase();
  for (const [id, catalog] of catalogEntries()) {
    if (catalog.macApps.some((macApp) => normalized.includes(macApp.toLowerCase()))) return id;
  }
  return null;
}

async function macToolboxIde(id: IdeId, bundlePath: string): Promise<DetectedIde> {
  const launcherPath = await macJetbrainsLauncher(bundlePath);
  if (launcherPath) {
    return { id, name: IDE_CATALOG[id].name, launcherPath, source: 'toolbox' };
  }
  return {
    id,
    name: IDE_CATALOG[id].name,
    launcherPath: '/usr/bin/open',
    bundlePath,
    source: 'toolbox',
  };
}

async function macJetbrainsLauncher(bundlePath: string): Promise<string | null> {
  let info: Record<string, unknown>;
  try {
    info = JSON.parse(
      await fs.readFile(path.join(bundlePath, 'Contents', 'Resources', 'product-info.json'), 'utf8')
    ) as Record<string, unknown>;
  } catch {
    return null;
  }
  const relative = productInfoLaunchPath(info);
  if (!relative) return null;
  for (const candidate of jetbrainsLauncherCandidates(bundlePath, relative)) {
    if (await isExecutable(candidate)) return candidate;
  }
  return null;
}

export function productInfoLaunchPath(info: Record<string, unknown>): string | null {
  return launchEntryPath(info.launch, 'launcherPath') ?? launchEntryPath(info.launchInfo, 'path');
}

function launchEntryPath(entries: unknown, pathKey: string): string | null {
  const candidates = Array.isArray(entries) ? entries : [entries];
  for (const candidate of candidates) {
    if (typeof candidate === 'string') return candidate;
    if (typeof candidate !== 'object' || candidate === null) continue;
    const record = candidate as Record<string, unknown>;
    if (typeof record.os === 'string' && !record.os.toLowerCase().includes('mac')) continue;
    const entryPath = record[pathKey];
    if (typeof entryPath === 'string' && entryPath.length > 0) return entryPath;
  }
  return null;
}

function jetbrainsLauncherCandidates(bundlePath: string, relative: string): string[] {
  if (path.isAbsolute(relative)) return [relative];
  const segments = relative.split('/').filter((segment) => segment.length > 0);
  return [path.join(bundlePath, 'Contents', ...segments), path.join(bundlePath, ...segments)];
}

function toolboxAppsRoot(): string {
  switch (process.platform) {
    case 'darwin':
      return path.join(
        os.homedir(),
        'Library',
        'Application Support',
        'JetBrains',
        'Toolbox',
        'apps'
      );
    case 'win32':
      return path.join(
        process.env.LOCALAPPDATA ?? path.join(os.homedir(), 'AppData', 'Local'),
        'JetBrains',
        'Toolbox',
        'apps'
      );
    default:
      return path.join(os.homedir(), '.local', 'share', 'JetBrains', 'Toolbox', 'apps');
  }
}

async function detectToolboxScripts(): Promise<DetectedIde[]> {
  const scriptExtension = process.platform === 'win32' ? '.exe' : '.sh';
  const directories = await walkDirectories(toolboxAppsRoot(), 5);
  const launchersByIde = new Map<IdeId, string>();
  for (const dir of directories) {
    let fileNames: string[];
    try {
      fileNames = await fs.readdir(dir);
    } catch {
      continue;
    }
    for (const fileName of fileNames) {
      if (!fileName.toLowerCase().endsWith(scriptExtension)) continue;
      const id = toolboxScriptToId(fileName);
      if (id && !launchersByIde.has(id)) launchersByIde.set(id, path.join(dir, fileName));
    }
  }
  return [...launchersByIde.entries()].map(([id, launcherPath]) => ({
    id,
    name: IDE_CATALOG[id].name,
    launcherPath,
    source: 'toolbox' as const,
  }));
}
