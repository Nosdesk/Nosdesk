import type { PluginTheme } from '@nosdesk/plugin-sdk';

// The plugin design-token contract: the `--nd-*` name a plugin sees, mapped to
// the host CSS variable it snapshots from (frontend/src/assets/main.css). A
// curated, stable subset, NOT a dump of every host token, so it stays a
// deliberate contract. Keep it in sync with the `--nd-*` table in
// docs/plugin-design-tokens.md and the fallbacks in the runtime kit CSS.
const TOKEN_MAP: Record<string, string> = {
  surface: '--color-surface',
  'surface-alt': '--color-surface-alt',
  'surface-hover': '--color-surface-hover',
  'app-bg': '--color-app',
  border: '--color-default',
  'border-subtle': '--color-subtle',
  'border-strong': '--color-strong',
  text: '--color-primary',
  'text-secondary': '--color-secondary',
  'text-tertiary': '--color-tertiary',
  accent: '--color-accent',
  'accent-hover': '--color-accent-hover',
  'on-accent': '--color-on-accent',
  success: '--color-status-success',
  'success-muted': '--color-status-success-muted',
  error: '--color-status-error',
  'error-muted': '--color-status-error-muted',
  warning: '--color-status-warning',
  'warning-muted': '--color-status-warning-muted',
  info: '--color-status-info',
  'info-muted': '--color-status-info-muted',
  'font-sans': '--font-sans',
  'font-mono': '--font-mono',
};

/**
 * Snapshot the host's active design tokens for a plugin sandbox. Reads the
 * RESOLVED values off `:root`, so it captures whatever theme is applied
 * (light / dark / named), and the runtime injects them as `--nd-*` variables.
 * `colorScheme` / `name` come from the theme store (the caller supplies them so
 * this stays store-agnostic).
 */
export function snapshotPluginTheme(colorScheme: 'light' | 'dark', name: string): PluginTheme {
  const cs = getComputedStyle(document.documentElement);
  const tokens: Record<string, string> = {};
  for (const [ndKey, hostVar] of Object.entries(TOKEN_MAP)) {
    const value = cs.getPropertyValue(hostVar).trim();
    if (value) tokens[ndKey] = value;
  }
  return { tokens, colorScheme, name };
}
