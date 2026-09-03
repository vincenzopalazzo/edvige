import { describe, expect, it } from 'vitest';
import { catppuccinFlavors, getCatppuccinAccentColor } from './catppuccin';
import { getChatChromeTokens } from './chat-chrome';
import { applyThemeTokens } from './theme-tokens';
import { CATPPUCCIN_THEME_IDS } from './types';

describe('chat chrome tokens', () => {
  it('keeps the built-in dark and aura bubble colors', () => {
    expect(getChatChromeTokens('dark')).toMatchObject({
      '--user-message-bubble-background': '#171d30',
      '--user-message-bubble-foreground': '#d2dae6',
      '--agent-message-bubble-background': '#1f2126',
    });
    expect(getChatChromeTokens('aura')).toMatchObject({
      '--user-message-bubble-background': '#393647',
      '--user-message-bubble-foreground': '#edecee',
    });
    expect(getChatChromeTokens('light')).toMatchObject({
      '--user-message-bubble-background': 'var(--color-text-primary)',
      '--user-message-bubble-foreground': 'var(--color-background-primary)',
      '--agent-message-bubble-background': 'transparent',
    });
  });

  it('maps each Catppuccin flavor onto its own bubble surfaces', () => {
    for (const flavorId of CATPPUCCIN_THEME_IDS) {
      const { palette, variant } = catppuccinFlavors[flavorId];
      const chrome = getChatChromeTokens(flavorId, 'teal');

      if (variant === 'light') {
        expect(chrome['--user-message-bubble-background']).toBe(palette.text);
        expect(chrome['--user-message-bubble-foreground']).toBe(palette.base);
        expect(chrome['--agent-message-bubble-background']).toBe(palette.mantle);
      } else {
        expect(chrome['--user-message-bubble-background']).toBe(palette.surface1);
        expect(chrome['--user-message-bubble-foreground']).toBe(palette.text);
        expect(chrome['--agent-message-bubble-background']).toBe(palette.surface0);
      }

      expect(chrome['--user-message-link-color']).toBe(getCatppuccinAccentColor(flavorId, 'teal'));
      expect(chrome['--user-message-bubble-background']).not.toBe('#171d30');
      expect(chrome['--agent-message-bubble-background']).not.toBe('#1f2126');
    }
  });

  it('applies chat chrome to the document root with the theme', () => {
    applyThemeTokens('catppuccin-mocha', 'blue');
    expect(
      document.documentElement.style.getPropertyValue('--user-message-bubble-background')
    ).toBe(catppuccinFlavors['catppuccin-mocha'].palette.surface1);
    expect(
      document.documentElement.style.getPropertyValue('--agent-message-bubble-background')
    ).toBe(catppuccinFlavors['catppuccin-mocha'].palette.surface0);
    expect(document.documentElement.style.getPropertyValue('--user-message-link-color')).toBe(
      getCatppuccinAccentColor('catppuccin-mocha', 'blue')
    );
  });
});
