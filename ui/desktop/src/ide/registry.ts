import type { IdeId } from '../types/ide';

export interface CatalogEntry {
  name: string;
  cliNames: string[];
  macApps: string[];
  jetbrains?: boolean;
}

export const IDE_CATALOG: Record<IdeId, CatalogEntry> = {
  zed: { name: 'Zed', cliNames: ['zed'], macApps: ['Zed.app'] },
  vscode: { name: 'VS Code', cliNames: ['code'], macApps: ['Visual Studio Code.app'] },
  'vscode-insiders': {
    name: 'VS Code Insiders',
    cliNames: ['code-insiders'],
    macApps: ['Visual Studio Code - Insiders.app'],
  },
  vscodium: { name: 'VSCodium', cliNames: ['codium'], macApps: ['VSCodium.app'] },
  cursor: { name: 'Cursor', cliNames: ['cursor'], macApps: ['Cursor.app'] },
  windsurf: { name: 'Windsurf', cliNames: ['windsurf'], macApps: ['Windsurf.app'] },
  sublime: { name: 'Sublime Text', cliNames: ['subl'], macApps: ['Sublime Text.app'] },
  xcode: { name: 'Xcode', cliNames: [], macApps: ['Xcode.app'] },
  'intellij-idea': {
    name: 'IntelliJ IDEA',
    cliNames: ['idea'],
    macApps: ['IntelliJ IDEA.app', 'IntelliJ IDEA Ultimate.app', 'IntelliJ IDEA CE.app'],
    jetbrains: true,
  },
  pycharm: {
    name: 'PyCharm',
    cliNames: ['pycharm'],
    macApps: ['PyCharm.app', 'PyCharm Professional.app', 'PyCharm CE.app'],
    jetbrains: true,
  },
  webstorm: {
    name: 'WebStorm',
    cliNames: ['webstorm'],
    macApps: ['WebStorm.app'],
    jetbrains: true,
  },
  goland: { name: 'GoLand', cliNames: ['goland'], macApps: ['GoLand.app'], jetbrains: true },
  clion: { name: 'CLion', cliNames: ['clion'], macApps: ['CLion.app'], jetbrains: true },
  rider: { name: 'Rider', cliNames: ['rider'], macApps: ['Rider.app'], jetbrains: true },
  rubymine: {
    name: 'RubyMine',
    cliNames: ['rubymine'],
    macApps: ['RubyMine.app'],
    jetbrains: true,
  },
  phpstorm: {
    name: 'PhpStorm',
    cliNames: ['phpstorm'],
    macApps: ['PhpStorm.app'],
    jetbrains: true,
  },
};
