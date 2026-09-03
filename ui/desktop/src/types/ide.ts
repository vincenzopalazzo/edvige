export type IdeId =
  | 'zed'
  | 'vscode'
  | 'vscode-insiders'
  | 'vscodium'
  | 'cursor'
  | 'windsurf'
  | 'sublime'
  | 'xcode'
  | 'intellij-idea'
  | 'pycharm'
  | 'webstorm'
  | 'goland'
  | 'clion'
  | 'rider'
  | 'rubymine'
  | 'phpstorm';

export type DetectionSource = 'path' | 'bundle' | 'toolbox' | 'desktop-file';

export interface DetectedIde {
  id: IdeId;
  name: string;
  launcherPath: string;
  launcherArgs?: string[];
  bundlePath?: string;
  source: DetectionSource;
}

export interface OpenTarget {
  dir: string;
  file?: string;
  line?: number;
  col?: number;
}

export function isOpenTarget(value: unknown): value is OpenTarget {
  if (typeof value !== 'object' || value === null) return false;
  const target = value as Record<string, unknown>;
  if (typeof target.dir !== 'string' || target.dir.trim() === '') return false;
  if (target.file !== undefined && (typeof target.file !== 'string' || target.file.trim() === '')) {
    return false;
  }
  if (
    target.line !== undefined &&
    (!Number.isInteger(target.line) || (target.line as number) < 1)
  ) {
    return false;
  }
  if (target.col !== undefined && (!Number.isInteger(target.col) || (target.col as number) < 1)) {
    return false;
  }
  return true;
}
