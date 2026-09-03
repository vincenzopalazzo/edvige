import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { dedupeIdes, parseDesktopExec, productInfoLaunchPath, toolboxScriptToId } from './detect';
import { buildPositionalArgs, buildSpawnArgs, spawnCommand } from './launch';
import { IDE_CATALOG } from './registry';
import { isOpenTarget, type DetectedIde, type IdeId } from '../types/ide';

const DIR = path.resolve('/tmp/project');
const FILE = path.join(DIR, 'src', 'app.ts');

function ideOf(id: IdeId, overrides: Partial<DetectedIde> = {}): DetectedIde {
  return {
    id,
    name: IDE_CATALOG[id].name,
    launcherPath: `/usr/local/bin/${IDE_CATALOG[id].cliNames[0] ?? id}`,
    source: 'path',
    ...overrides,
  };
}

describe('parseDesktopExec', () => {
  it('splits the executable from fixed arguments and drops field codes', () => {
    expect(parseDesktopExec('code %U')).toEqual({ path: 'code', args: [] });
    expect(parseDesktopExec('/usr/bin/jetbrains-idea %f')).toEqual({
      path: '/usr/bin/jetbrains-idea',
      args: [],
    });
    expect(parseDesktopExec(' codium --new-window %F ')).toEqual({
      path: 'codium',
      args: ['--new-window'],
    });
  });

  it('keeps flatpak launcher arguments', () => {
    expect(parseDesktopExec('/usr/bin/flatpak run com.visualstudio.code %F')).toEqual({
      path: '/usr/bin/flatpak',
      args: ['run', 'com.visualstudio.code'],
    });
  });

  it('strips surrounding quotes from quoted executables', () => {
    expect(parseDesktopExec('"/opt/My App/bin/run" --wait %f')).toEqual({
      path: '/opt/My App/bin/run',
      args: ['--wait'],
    });
  });

  it('returns an empty path when only field codes remain', () => {
    expect(parseDesktopExec('%f %U')).toEqual({ path: '', args: [] });
  });
});

describe('toolboxScriptToId', () => {
  it('maps known launcher scripts to JetBrains ids', () => {
    expect(toolboxScriptToId('idea.sh')).toBe('intellij-idea');
    expect(toolboxScriptToId('pycharm.sh')).toBe('pycharm');
    expect(toolboxScriptToId('goland64.exe')).toBe('goland');
  });

  it('is case insensitive and rejects unknown scripts', () => {
    expect(toolboxScriptToId('WEBSTORM.SH')).toBe('webstorm');
    expect(toolboxScriptToId('code.sh')).toBeNull();
    expect(toolboxScriptToId('idea')).toBeNull();
  });
});

describe('productInfoLaunchPath', () => {
  it('reads the mac launcher from the launch array', () => {
    const info = {
      launch: [
        { launcherPath: 'bin/idea.sh', os: 'Linux' },
        { launcherPath: 'MacOS/idea', os: 'Mac OS X' },
        { launcherPath: 'bin\\idea64.exe', os: 'Windows' },
      ],
    };
    expect(productInfoLaunchPath(info)).toBe('MacOS/idea');
  });

  it('falls back to the legacy launchInfo field', () => {
    expect(productInfoLaunchPath({ launchInfo: [{ os: 'Mac', path: 'MacOS/idea' }] })).toBe(
      'MacOS/idea'
    );
    expect(productInfoLaunchPath({ launchInfo: 'bin/idea.sh' })).toBe('bin/idea.sh');
  });

  it('returns null when no mac launcher exists', () => {
    expect(productInfoLaunchPath({})).toBeNull();
    expect(
      productInfoLaunchPath({ launch: [{ launcherPath: 'bin/idea.sh', os: 'Linux' }] })
    ).toBeNull();
  });
});

describe('buildPositionalArgs', () => {
  it('passes the directory when no file is given', () => {
    expect(buildPositionalArgs(ideOf('zed'), { dir: DIR })).toEqual([DIR]);
  });

  it('appends line and column positions to the file', () => {
    expect(buildPositionalArgs(ideOf('zed'), { dir: DIR, file: FILE, line: 12, col: 5 })).toEqual([
      `${FILE}:12:5`,
    ]);
    expect(buildPositionalArgs(ideOf('sublime'), { dir: DIR, file: FILE, line: 3 })).toEqual([
      `${FILE}:3`,
    ]);
  });

  it('ignores column when line is missing', () => {
    expect(buildPositionalArgs(ideOf('sublime'), { dir: DIR, file: FILE, col: 7 })).toEqual([FILE]);
  });
});

describe('buildSpawnArgs', () => {
  it('uses -g plus the trailing directory for the VS Code family', () => {
    expect(buildSpawnArgs(ideOf('vscode'), { dir: DIR, file: FILE, line: 10, col: 2 })).toEqual([
      '-g',
      `${FILE}:10:2`,
      DIR,
    ]);
    expect(buildSpawnArgs(ideOf('cursor'), { dir: DIR })).toEqual([DIR]);
    expect(buildSpawnArgs(ideOf('windsurf'), { dir: DIR, file: FILE })).toEqual(['-g', FILE, DIR]);
  });

  it('passes a single positional for zed and sublime', () => {
    expect(buildSpawnArgs(ideOf('zed'), { dir: DIR, file: FILE, line: 10, col: 2 })).toEqual([
      `${FILE}:10:2`,
    ]);
    expect(buildSpawnArgs(ideOf('sublime'), { dir: DIR })).toEqual([DIR]);
  });

  it('maps files to --line for JetBrains IDEs and ignores column', () => {
    expect(
      buildSpawnArgs(ideOf('intellij-idea'), { dir: DIR, file: FILE, line: 42, col: 9 })
    ).toEqual(['--line', '42', FILE]);
    expect(buildSpawnArgs(ideOf('pycharm'), { dir: DIR, file: FILE })).toEqual([
      '--line',
      '1',
      FILE,
    ]);
    expect(buildSpawnArgs(ideOf('rider'), { dir: DIR })).toEqual([DIR]);
  });

  it('falls back to open -a for macOS bundles without a CLI', () => {
    const xcode = ideOf('xcode', {
      launcherPath: '/usr/bin/open',
      bundlePath: '/Applications/Xcode.app',
      source: 'bundle',
    });
    expect(buildSpawnArgs(xcode, { dir: DIR, file: FILE, line: 4 })).toEqual([
      '-a',
      '/Applications/Xcode.app',
      FILE,
    ]);
    expect(buildSpawnArgs(xcode, { dir: DIR })).toEqual(['-a', '/Applications/Xcode.app', DIR]);
  });
});

describe('isOpenTarget', () => {
  it('requires a non-empty directory', () => {
    expect(isOpenTarget({ dir: DIR })).toBe(true);
    expect(isOpenTarget({ dir: '' })).toBe(false);
    expect(isOpenTarget({})).toBe(false);
    expect(isOpenTarget(null)).toBe(false);
  });

  it('rejects empty files and non-positive positions', () => {
    expect(isOpenTarget({ dir: DIR, file: FILE, line: 1, col: 1 })).toBe(true);
    expect(isOpenTarget({ dir: DIR, file: '' })).toBe(false);
    expect(isOpenTarget({ dir: DIR, line: 0 })).toBe(false);
    expect(isOpenTarget({ dir: DIR, col: 1.5 })).toBe(false);
  });
});

describe('spawnCommand', () => {
  it('launches the detected binary directly on non-Windows', () => {
    const ide = ideOf('vscode');
    expect(spawnCommand(ide, [DIR])).toEqual({ command: ide.launcherPath, args: [DIR] });
  });

  it('prepends fixed launcher arguments from desktop entries', () => {
    const flatpakVscode = ideOf('vscode', {
      launcherPath: '/usr/bin/flatpak',
      launcherArgs: ['run', 'com.visualstudio.code'],
      source: 'desktop-file',
    });
    expect(spawnCommand(flatpakVscode, ['-g', FILE, DIR])).toEqual({
      command: '/usr/bin/flatpak',
      args: ['run', 'com.visualstudio.code', '-g', FILE, DIR],
    });
  });
});

describe('dedupeIdes', () => {
  it('keeps the highest-priority source per id', () => {
    const viaPath = ideOf('vscode', { launcherPath: '/usr/local/bin/code' });
    const viaDesktopFile = ideOf('vscode', {
      launcherPath: '/snap/bin/code',
      source: 'desktop-file',
    });
    const result = dedupeIdes([viaDesktopFile, viaPath]);
    expect(result).toHaveLength(1);
    expect(result[0]).toMatchObject({
      id: 'vscode',
      source: 'path',
      launcherPath: '/usr/local/bin/code',
    });
  });

  it('sorts survivors alphabetically by catalog name', () => {
    const result = dedupeIdes([ideOf('zed'), ideOf('vscode'), ideOf('intellij-idea')]);
    expect(result.map((ide) => ide.id)).toEqual(['intellij-idea', 'vscode', 'zed']);
  });
});
