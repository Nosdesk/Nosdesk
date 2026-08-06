// The base UI kit injected into every plugin sandbox document. Static (authored
// here, bundled into runtime.js); only the `--nd-*` token VALUES are dynamic and
// pushed by the host (see injectTokens in runtime.ts). Everything references
// `var(--nd-*, <fallback>)` so it still renders sanely before the first token
// push. This is what gives a plugin's raw DOM the app's fonts, colours,
// scrollbars, buttons, and inputs without the plugin reimplementing them.
//
// The classes are the plugin contract: `.nd-btn` / `.nd-btn--primary`,
// `.nd-input`, `.nd-textarea`, `.nd-card`, `.nd-label`, `.nd-muted`. Keep them
// stable; they are versioned with the runtime.
//
// The OUTER card is the host's, not the plugin's. On a `card`-chrome slot the
// host wraps this document in the app's `SectionCard` and supplies the border,
// radius, header and body padding, so plugin content should start flush at the
// top-left and add no outer frame of its own. See `.nd-card` below.

export const PLUGIN_UI_CSS = `
:root { --nd-radius: 8px; color-scheme: light dark; }

/* Baseline reset: border-box so a border/padding never widens an element past
   its width (the classic source of a stray 100%+2px horizontal scrollbar in a
   width-constrained panel). */
*, *::before, *::after { box-sizing: border-box; }

html, body { margin: 0; padding: 0; }
/* The host sizes the iframe to the plugin's reported content height, so the
   document itself is never a horizontal scroll container: clip rather than show
   a spurious x-scrollbar. Vertical stays visible so nothing is silently hidden
   if a height report ever lags. */
body { overflow-x: hidden; }
body {
  font-family: var(--nd-font-sans, system-ui, -apple-system, sans-serif);
  font-size: 13px;
  line-height: 1.5;
  color: var(--nd-text, #1f2937);
  background: transparent;
  -webkit-font-smoothing: antialiased;
}

/* Scrollbars matched to the app's subtle style. */
* { scrollbar-width: thin; scrollbar-color: var(--nd-border-strong, #cbd5e1) transparent; }
*::-webkit-scrollbar { width: 8px; height: 8px; }
*::-webkit-scrollbar-thumb { background: var(--nd-border-strong, #cbd5e1); border-radius: 4px; }
*::-webkit-scrollbar-thumb:hover { background: var(--nd-text-tertiary, #9ca3af); }
*::-webkit-scrollbar-track { background: transparent; }

a { color: var(--nd-accent, #FF6B1A); }

.nd-btn {
  font: inherit;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 12px;
  border-radius: var(--nd-radius, 8px);
  border: 1px solid var(--nd-border, #e5e7eb);
  background: var(--nd-surface, #ffffff);
  color: var(--nd-text, #1f2937);
  font-weight: 500;
  cursor: pointer;
  transition: background 0.12s ease, border-color 0.12s ease;
}
.nd-btn:hover { background: var(--nd-surface-hover, #f3f4f6); }
.nd-btn:disabled { opacity: 0.5; cursor: not-allowed; }
.nd-btn--primary {
  background: var(--nd-accent, #FF6B1A);
  border-color: var(--nd-accent, #FF6B1A);
  color: var(--nd-on-accent, #000000);
}
.nd-btn--primary:hover {
  background: var(--nd-accent-hover, #EB5808);
  border-color: var(--nd-accent-hover, #EB5808);
}

.nd-input, .nd-textarea {
  font: inherit;
  width: 100%;
  box-sizing: border-box;
  padding: 6px 8px;
  border: 1px solid var(--nd-border, #e5e7eb);
  border-radius: var(--nd-radius, 8px);
  background: var(--nd-surface, #ffffff);
  color: var(--nd-text, #1f2937);
}
.nd-input::placeholder, .nd-textarea::placeholder { color: var(--nd-text-tertiary, #9ca3af); }
.nd-input:focus, .nd-textarea:focus {
  outline: none;
  border-color: var(--nd-accent, #FF6B1A);
}
.nd-textarea { resize: vertical; }

/* An INNER sub-card, for grouping content inside a panel.
 *
 * This is deliberately not the app's outer card. A panel on a \`card\`-chrome
 * slot is already wrapped by the host in the real \`SectionCard\` (border,
 * radius, surface, header pill, 12px body padding), so a plugin that drew the
 * outer card itself would produce a card inside a card. Use \`.nd-card\` only to
 * subdivide, and use the surface-alt background so it reads as recessed
 * against the host card it sits in.
 *
 * A contribution that genuinely needs to own its outer frame declares
 * \`"chrome": "none"\` in its manifest and gets a bare, unwrapped iframe. */
.nd-card {
  border: 1px solid var(--nd-border, #e5e7eb);
  border-radius: var(--nd-radius-sm, 6px);
  background: var(--nd-surface-alt, #f9fafb);
  padding: var(--nd-space-sm, 8px);
}

.nd-label { font-weight: 600; color: var(--nd-text, #1f2937); }
.nd-muted { color: var(--nd-text-tertiary, #6b7280); }
`;
