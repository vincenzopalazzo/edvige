import { catppuccinFlavors } from './catppuccin';
import {
  DEFAULT_CATPPUCCIN_ACCENT,
  isCatppuccinThemeId,
  type CatppuccinAccent,
  type CatppuccinThemeId,
  type ThemeId,
} from './types';

// Goose-only chat chrome. These are not MCP semantic tokens — message bubbles
// are desktop UI, and the old .dark hex overrides leaked into every dark theme.
export type ChatChromeTokens = {
  '--user-message-bubble-background': string;
  '--user-message-bubble-foreground': string;
  '--user-message-code-background': string;
  '--user-message-heading-color': string;
  '--user-message-list-color': string;
  '--user-message-marker-color': string;
  '--user-message-link-color': string;
  '--user-message-emphasis-color': string;
  '--agent-message-bubble-background': string;
  '--agent-message-bubble-radius': string;
  '--agent-message-bubble-padding': string;
  '--message-inline-code-background': string;
  '--message-inline-code-color': string;
  '--message-inline-code-padding': string;
  '--message-pre-code-color': string;
};

const darkMarkdownChrome = {
  '--user-message-code-background': '#0f1628',
  '--user-message-heading-color': '#e7ebf2',
  '--user-message-list-color': '#c4cfde',
  '--user-message-marker-color': '#8798b0',
  '--user-message-link-color': '#aecfe3',
  '--user-message-emphasis-color': '#c6d9df',
  '--agent-message-bubble-background': '#1f2126',
  '--agent-message-bubble-radius': '0.75rem',
  '--agent-message-bubble-padding': '0.625rem 1rem',
  '--message-inline-code-background': 'transparent',
  '--message-inline-code-color': '#8cd3d6',
  '--message-inline-code-padding': '0',
  '--message-pre-code-color': '#b8c4d1',
} as const;

const lightChatChrome: ChatChromeTokens = {
  '--user-message-bubble-background': 'var(--color-text-primary)',
  '--user-message-bubble-foreground': 'var(--color-background-primary)',
  '--user-message-code-background': '#282c34',
  '--user-message-heading-color': 'inherit',
  '--user-message-list-color': 'inherit',
  '--user-message-marker-color': 'inherit',
  '--user-message-link-color': 'inherit',
  '--user-message-emphasis-color': 'inherit',
  '--agent-message-bubble-background': 'transparent',
  '--agent-message-bubble-radius': '0',
  '--agent-message-bubble-padding': '0',
  '--message-inline-code-background': 'var(--color-neutral-50)',
  '--message-inline-code-color': 'var(--color-inline-code)',
  '--message-inline-code-padding': '2px 4px',
  '--message-pre-code-color': 'inherit',
};

const darkChatChrome: ChatChromeTokens = {
  '--user-message-bubble-background': '#171d30',
  '--user-message-bubble-foreground': '#d2dae6',
  ...darkMarkdownChrome,
};

const auraChatChrome: ChatChromeTokens = {
  '--user-message-bubble-background': '#393647',
  '--user-message-bubble-foreground': '#edecee',
  ...darkMarkdownChrome,
};

function catppuccinChatChrome(
  flavorId: CatppuccinThemeId,
  accent: CatppuccinAccent
): ChatChromeTokens {
  const { palette, variant } = catppuccinFlavors[flavorId];
  const invertedUser = variant === 'light';

  return {
    '--user-message-bubble-background': invertedUser ? palette.text : palette.surface1,
    '--user-message-bubble-foreground': invertedUser ? palette.base : palette.text,
    '--user-message-code-background': palette.crust,
    '--user-message-heading-color': invertedUser ? palette.base : palette.text,
    '--user-message-list-color': invertedUser ? palette.base : palette.subtext1,
    '--user-message-marker-color': palette.overlay1,
    '--user-message-link-color': palette[accent],
    '--user-message-emphasis-color': invertedUser ? palette.base : palette.subtext0,
    '--agent-message-bubble-background': invertedUser ? palette.mantle : palette.surface0,
    '--agent-message-bubble-radius': '0.75rem',
    '--agent-message-bubble-padding': '0.625rem 1rem',
    '--message-inline-code-background': 'transparent',
    '--message-inline-code-color': palette.teal,
    '--message-inline-code-padding': '0',
    '--message-pre-code-color': invertedUser ? palette.base : palette.subtext1,
  };
}

export function getChatChromeTokens(
  themeId: ThemeId,
  accent: CatppuccinAccent = DEFAULT_CATPPUCCIN_ACCENT
): ChatChromeTokens {
  if (isCatppuccinThemeId(themeId)) {
    return catppuccinChatChrome(themeId, accent);
  }
  if (themeId === 'dark') {
    return darkChatChrome;
  }
  if (themeId === 'aura') {
    return auraChatChrome;
  }
  return lightChatChrome;
}
