import { spawn } from 'node:child_process';
import path from 'node:path';
import { IDE_CATALOG } from './registry';
import type { DetectedIde, IdeId, OpenTarget } from '../types/ide';

const VS_CODE_FAMILY = new Set<IdeId>([
  'vscode',
  'vscode-insiders',
  'vscodium',
  'cursor',
  'windsurf',
]);

function isOpenFallback(ide: DetectedIde): ide is DetectedIde & { bundlePath: string } {
  return path.basename(ide.launcherPath) === 'open' && ide.bundlePath !== undefined;
}

function quoteWindowsArg(arg: string): string {
  return `"${arg.replace(/"/g, '\\"')}"`;
}

export function spawnCommand(
  ide: DetectedIde,
  args: string[] = []
): { command: string; args: string[]; windowsVerbatimArguments?: boolean } {
  const fullArgs = [...(ide.launcherArgs ?? []), ...args];
  if (process.platform === 'win32' && /\.(cmd|bat)$/i.test(ide.launcherPath)) {
    const commandLine = [ide.launcherPath, ...fullArgs].map(quoteWindowsArg).join(' ');
    return {
      command: process.env.ComSpec || 'cmd.exe',
      args: ['/d', '/s', '/c', `"${commandLine}"`],
      windowsVerbatimArguments: true,
    };
  }
  return { command: ide.launcherPath, args: fullArgs };
}

function positionedFile(file: string, target: OpenTarget): string {
  let positioned = path.resolve(file);
  if (target.line !== undefined) positioned += `:${target.line}`;
  if (target.line !== undefined && target.col !== undefined) positioned += `:${target.col}`;
  return positioned;
}

export function buildPositionalArgs(_ide: DetectedIde, target: OpenTarget): string[] {
  if (target.file) return [positionedFile(target.file, target)];
  return [path.resolve(target.dir)];
}

export function buildSpawnArgs(ide: DetectedIde, target: OpenTarget): string[] {
  if (isOpenFallback(ide)) {
    return ['-a', ide.bundlePath, path.resolve(target.file ?? target.dir)];
  }
  if (VS_CODE_FAMILY.has(ide.id)) {
    if (!target.file) return [path.resolve(target.dir)];
    return ['-g', positionedFile(target.file, target), path.resolve(target.dir)];
  }
  if (IDE_CATALOG[ide.id].jetbrains) {
    if (!target.file) return [path.resolve(target.dir)];
    return ['--line', String(target.line ?? 1), path.resolve(target.file)];
  }
  return buildPositionalArgs(ide, target);
}

export async function openInIde(ide: DetectedIde, target: OpenTarget): Promise<void> {
  const { command, args, windowsVerbatimArguments } = spawnCommand(
    ide,
    buildSpawnArgs(ide, target)
  );
  await new Promise<void>((resolve, reject) => {
    const child = spawn(command, args, {
      detached: true,
      stdio: 'ignore',
      windowsHide: true,
      windowsVerbatimArguments,
    });
    child.once('error', reject);
    child.once('spawn', resolve);
    child.unref();
  });
}
