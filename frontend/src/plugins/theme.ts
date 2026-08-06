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

// Metric tokens. Unlike the colours above these are NOT read off `:root` —
// the host expresses them as Tailwind utilities (`rounded-xl`, `p-3`, `h-9`),
// so there is no custom property to snapshot and the values are transcribed
// here instead. They exist so guest content can match host conventions rather
// than guessing: the same radius scale, the same spacing rhythm, and the
// header metrics of the `SectionCard` the host draws around a `card`-chrome
// panel.
//
// Transcribed, so they can drift. If `SectionCard` changes its radius, header
// height or body padding, change these to match.
const STATIC_TOKENS: Record<string, string> = {
  // Radius scale. `radius` stays the general-purpose value a plugin gets for
  // buttons and inputs; `radius-lg` is the card radius (`rounded-xl`).
  radius: '8px',
  'radius-sm': '6px',
  'radius-lg': '12px',
  // Spacing rhythm. `space-md` (12px) is the host card's body padding, so a
  // plugin separating its own sections by `space-md` matches the surrounding
  // vertical rhythm exactly.
  'space-xs': '4px',
  'space-sm': '8px',
  'space-md': '12px',
  'space-lg': '16px',
  // `SectionCard` header pill: 36px tall (`h-9`), 12px inline padding
  // (`px-3`), 13px semibold title. A plugin drawing its own header (under
  // `chrome: "none"`) can reproduce the host's exactly.
  'header-height': '36px',
  'header-padding-inline': '12px',
  'header-font-size': '13px',
  'header-font-weight': '600',
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
  const tokens: Record<string, string> = { ...STATIC_TOKENS };
  for (const [ndKey, hostVar] of Object.entries(TOKEN_MAP)) {
    const value = cs.getPropertyValue(hostVar).trim();
    if (value) tokens[ndKey] = value;
  }
  return { tokens, colorScheme, name };
}
