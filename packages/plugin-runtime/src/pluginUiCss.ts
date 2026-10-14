// The base UI kit injected into every plugin sandbox document. Static (authored
// here, bundled into runtime.js); only the `--nd-*` token VALUES are dynamic and
// pushed by the host (see injectTokens in runtime.ts). Everything references
// `var(--nd-*, <fallback>)` so it still renders sanely before the first token
// push. This is what gives a plugin's raw DOM the app's fonts, colours,
// scrollbars, buttons, and inputs without the plugin reimplementing them.
//
// The class NAMES are the plugin contract, versioned with the runtime: never
// remove or rename one, only add. Their look tracks the app. Each block copies
// its metrics from the app component named next to it (Button, FormInput,
// FormTextarea, BaseDropdown, Checkbox, ToggleSwitch, StatusPill, DataTable,
// the tabs recipe, Callout, EmptyState), so a plugin's controls sit next to
// the app's without a visible seam. The original names are `.nd-btn` /
// `.nd-btn--primary`, `.nd-input`, `.nd-textarea`, `.nd-card`, `.nd-label`,
// `.nd-muted`.
//
// Only `--nd-*` names from the host contract are referenced (TOKEN_MAP and the
// metric tokens in frontend/src/plugins/theme.ts). Tints the contract has no
// token for (the accent tint, the 30% pill border, the callout wash) are
// derived with color-mix() from those tokens rather than adding a name.
//
// The OUTER card is the host's, not the plugin's. On a `card`-chrome slot the
// host wraps this document in the app's `SectionCard` and supplies the border,
// radius, header and body padding, so plugin content should start flush at the
// top-left and add no outer frame of its own. See `.nd-card` below.

export const PLUGIN_UI_CSS = `
:root { --nd-radius: 8px; color-scheme: light dark; }
/* Follow the host's light or dark choice exactly once it is known, so native
   control parts (select popups, checkbox and radio glyphs) render in the same
   scheme as the tokens rather than whatever the OS prefers. */
:root[data-nd-color-scheme="light"] { color-scheme: light; }
:root[data-nd-color-scheme="dark"] { color-scheme: dark; }

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
/* 14px on a 20px line is the app's body text (\`text-sm\`): the values in a
   SectionCard, table cells, md buttons and form controls all use it. */
body {
  font-family: var(--nd-font-sans, system-ui, -apple-system, sans-serif);
  font-size: 14px;
  line-height: 1.4286;
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

/* Buttons: the app's button recipe (frontend/src/recipes/button.ts), md
   size. The default \`.nd-btn\` is the app's secondary button: surface-alt fill
   with the default border. Filled and ghost variants have no border, as in the
   app, so they are 2px shorter than the bordered default exactly like the
   app's buttons are. */
.nd-btn {
  font: inherit;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 8px 16px;
  border-radius: var(--nd-radius, 8px);
  border: 1px solid var(--nd-border, #e5e7eb);
  background: var(--nd-surface-alt, #f3f4f6);
  color: var(--nd-text, #1f2937);
  font-size: 14px;
  font-weight: 500;
  line-height: 20px;
  white-space: nowrap;
  cursor: pointer;
  transition: background-color 0.15s ease, border-color 0.15s ease, color 0.15s ease, opacity 0.15s ease;
}
.nd-btn:hover { background: var(--nd-surface-hover, #e5e7eb); }
.nd-btn:disabled { opacity: 0.5; cursor: not-allowed; }
.nd-btn--primary {
  background: var(--nd-accent, #FF6B1A);
  border-color: var(--nd-accent, #FF6B1A);
  border-width: 0;
  color: var(--nd-on-accent, #000000);
}
.nd-btn--primary:hover {
  background: var(--nd-accent-hover, #EB5808);
  border-color: var(--nd-accent-hover, #EB5808);
}

/* Text inputs: the app's FormInput and FormTextarea. A surface-alt fill with a
   subtle border, 8px 12px padding at 14px (38px tall), and on focus an accent
   border with a 2px accent ring. The textarea's border steps up on hover. */
.nd-input, .nd-textarea {
  font: inherit;
  width: 100%;
  box-sizing: border-box;
  padding: 8px 12px;
  border: 1px solid var(--nd-border-subtle, #f3f4f6);
  border-radius: var(--nd-radius, 8px);
  background: var(--nd-surface-alt, #f3f4f6);
  color: var(--nd-text, #1f2937);
  font-size: 14px;
  line-height: 20px;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}
.nd-input::placeholder, .nd-textarea::placeholder { color: var(--nd-text-tertiary, #6b7280); opacity: 1; }
.nd-input:focus, .nd-textarea:focus { outline: none; }
.nd-input:focus-visible, .nd-textarea:focus-visible {
  border-color: var(--nd-accent, #FF6B1A);
  box-shadow: 0 0 0 2px var(--nd-accent, #FF6B1A);
}
.nd-textarea { resize: vertical; }
.nd-textarea:hover:not(:disabled):not(:focus-visible):not([aria-invalid="true"]) { border-color: var(--nd-border, #e5e7eb); }

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

/* Button variants, size, focus and disabled. \`.nd-btn--secondary\` spells
   out the default look for markup that wants to name it. */
.nd-btn--secondary {
  background: var(--nd-surface-alt, #f3f4f6);
  border-color: var(--nd-border, #e5e7eb);
  color: var(--nd-text, #1f2937);
}
.nd-btn--secondary:hover { background: var(--nd-surface-hover, #e5e7eb); }
.nd-btn--danger {
  background: var(--nd-error, #EF4444);
  border-width: 0;
  color: #ffffff;
}
.nd-btn--danger:hover { background: var(--nd-error, #EF4444); }
.nd-btn--danger:hover:not(:disabled) { opacity: 0.9; }
.nd-btn--ghost {
  background: transparent;
  border-width: 0;
  color: var(--nd-text-secondary, #4b5563);
}
.nd-btn--ghost:hover {
  background: var(--nd-surface-hover, #e5e7eb);
  color: var(--nd-text, #1f2937);
}
.nd-btn--sm { font-size: 12px; line-height: 16px; padding: 6px 12px; gap: 6px; }
.nd-btn:focus-visible {
  outline: none;
  box-shadow: 0 0 0 2px var(--nd-surface, #ffffff), 0 0 0 4px var(--nd-accent, #FF6B1A);
}
.nd-btn--ghost:disabled:hover { background: transparent; color: var(--nd-text-secondary, #4b5563); }

/* Native select: the app's dropdown trigger (BaseDropdown, sm, the size used
   almost everywhere). Surface-alt fill, subtle border, 6px 12px at 14px (34px
   tall). The chevron is two gradient strokes in the tertiary text colour, so
   it follows the theme without an image. Hover steps the border up; focus is
   an accent border with a 1px accent ring. */
.nd-select {
  font: inherit;
  width: 100%;
  padding: 6px 36px 6px 12px;
  border: 1px solid var(--nd-border-subtle, #f3f4f6);
  border-radius: var(--nd-radius, 8px);
  background-color: var(--nd-surface-alt, #f3f4f6);
  color: var(--nd-text, #1f2937);
  font-size: 14px;
  line-height: 20px;
  -webkit-appearance: none;
  appearance: none;
  background-image:
    linear-gradient(45deg, transparent calc(50% - 0.8px), var(--nd-text-tertiary, #6b7280) calc(50% - 0.8px), var(--nd-text-tertiary, #6b7280) calc(50% + 0.8px), transparent calc(50% + 0.8px)),
    linear-gradient(135deg, transparent calc(50% - 0.8px), var(--nd-text-tertiary, #6b7280) calc(50% - 0.8px), var(--nd-text-tertiary, #6b7280) calc(50% + 0.8px), transparent calc(50% + 0.8px));
  background-position: calc(100% - 20px) 50%, calc(100% - 14px) 50%;
  background-size: 6px 6px;
  background-repeat: no-repeat;
  cursor: pointer;
  transition: border-color 0.12s ease, box-shadow 0.12s ease;
}
.nd-select:hover:not(:disabled) { border-color: var(--nd-border-strong, #d1d5db); }
.nd-select:focus {
  outline: none;
  border-color: var(--nd-accent, #FF6B1A);
  box-shadow: 0 0 0 1px var(--nd-accent, #FF6B1A);
}
.nd-select[multiple], .nd-select[size] { padding-right: 12px; background-image: none; }

/* Inside the recessed \`.nd-card\` the fill would vanish into the card, so
   controls step back up to the surface there. */
.nd-card .nd-input, .nd-card .nd-textarea, .nd-card .nd-select {
  background-color: var(--nd-surface, #ffffff);
}
.nd-card :is(.nd-input, .nd-textarea, .nd-select):not(:focus):not(:hover):not([aria-invalid="true"]) {
  border-color: var(--nd-border, #e5e7eb);
}

/* Checkbox and radio stay native and take the accent through accent-color.
   16px is the app's Checkbox size. A label wrapping one lays out as the app's
   checkbox row does: control, 8px, text. */
.nd-checkbox, .nd-radio {
  width: 16px;
  height: 16px;
  margin: 0;
  flex-shrink: 0;
  vertical-align: middle;
  accent-color: var(--nd-accent, #FF6B1A);
  cursor: pointer;
}
label:has(> .nd-checkbox), label:has(> .nd-radio), label:has(> .nd-switch) {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
}

/* Switch: a native \`input type="checkbox" role="switch"\`, drawn to match the
   app's ToggleSwitch (md): 44 by 24 track with a 2px border, 20px white thumb. */
.nd-switch {
  -webkit-appearance: none;
  appearance: none;
  position: relative;
  display: inline-block;
  flex-shrink: 0;
  width: 44px;
  height: 24px;
  margin: 0;
  vertical-align: middle;
  border: 2px solid var(--nd-border-strong, #d1d5db);
  border-radius: 9999px;
  background: var(--nd-surface-alt, #f3f4f6);
  cursor: pointer;
  transition: background-color 0.2s ease, border-color 0.2s ease;
}
.nd-switch::before {
  content: "";
  position: absolute;
  top: 0;
  left: 0;
  width: 20px;
  height: 20px;
  border-radius: 9999px;
  background: #ffffff;
  box-shadow: 0 10px 15px -3px rgba(0, 0, 0, 0.1), 0 4px 6px -4px rgba(0, 0, 0, 0.1);
  transition: transform 0.2s ease;
}
.nd-switch:checked {
  background: var(--nd-accent, #FF6B1A);
  border-color: var(--nd-accent, #FF6B1A);
}
.nd-switch:checked::before { transform: translateX(20px); }
.nd-switch:hover:not(:disabled) { opacity: 0.9; }
@media (prefers-reduced-motion: reduce) {
  .nd-switch, .nd-switch::before { transition: none; }
}

.nd-checkbox:focus-visible, .nd-radio:focus-visible, .nd-switch:focus-visible {
  outline: 2px solid var(--nd-accent, #FF6B1A);
  outline-offset: 2px;
}
.nd-select:disabled, .nd-checkbox:disabled, .nd-radio:disabled, .nd-switch:disabled,
.nd-input:disabled, .nd-textarea:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
label:has(> :disabled) { cursor: not-allowed; }

/* A form field: caption, control, then help or error text, 6px apart. The
   caption is the app's form label (FormInput, BaseDropdown): 12px, medium,
   tertiary, uppercase. A label that wraps its own control (a checkbox row)
   keeps the plain look. Mark a bad control with aria-invalid="true". */
.nd-field { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
.nd-field > label:not(:has(input, select, textarea)) {
  font-size: 12px;
  line-height: 16px;
  font-weight: 500;
  color: var(--nd-text-tertiary, #6b7280);
  text-transform: uppercase;
  letter-spacing: 0.025em;
}
.nd-help, .nd-error { margin: 0; font-size: 12px; line-height: 16px; }
.nd-help { color: var(--nd-text-tertiary, #6b7280); }
.nd-error { color: var(--nd-error, #EF4444); }
.nd-input[aria-invalid="true"], .nd-textarea[aria-invalid="true"], .nd-select[aria-invalid="true"] {
  border-color: var(--nd-error, #EF4444);
}

/* Badge: the app's StatusPill (xs). A pill with a 4px leading dot so colour is
   never the only signal; \`.nd-badge--no-dot\` drops it. Each tone is its muted
   token behind its full-strength text, with a 30% tone border. */
.nd-badge {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 6px;
  border: 1px solid var(--nd-border, #e5e7eb);
  border-radius: 9999px;
  background: var(--nd-surface-hover, #e5e7eb);
  color: var(--nd-text-tertiary, #6b7280);
  font-size: 10px;
  font-weight: 600;
  line-height: 1;
  letter-spacing: 0.025em;
  text-transform: uppercase;
  white-space: nowrap;
  vertical-align: middle;
}
.nd-badge::before {
  content: "";
  flex-shrink: 0;
  width: 4px;
  height: 4px;
  border-radius: 9999px;
  background: currentColor;
}
.nd-badge--no-dot::before { content: none; }
.nd-badge--neutral {
  background: var(--nd-surface-hover, #e5e7eb);
  border-color: var(--nd-border, #e5e7eb);
  color: var(--nd-text-tertiary, #6b7280);
}
.nd-badge--accent {
  background: rgba(255, 107, 26, 0.15);
  background: color-mix(in srgb, var(--nd-accent, #FF6B1A) 15%, transparent);
  border-color: rgba(255, 107, 26, 0.3);
  border-color: color-mix(in srgb, var(--nd-accent, #FF6B1A) 30%, transparent);
  color: var(--nd-accent, #FF6B1A);
}
.nd-badge--success {
  background: var(--nd-success-muted, rgba(0, 201, 81, 0.15));
  border-color: rgba(0, 201, 81, 0.3);
  border-color: color-mix(in srgb, var(--nd-success, #00C951) 30%, transparent);
  color: var(--nd-success, #00C951);
}
.nd-badge--warning {
  background: var(--nd-warning-muted, rgba(245, 158, 11, 0.15));
  border-color: rgba(245, 158, 11, 0.3);
  border-color: color-mix(in srgb, var(--nd-warning, #F59E0B) 30%, transparent);
  color: var(--nd-warning, #F59E0B);
}
.nd-badge--error {
  background: var(--nd-error-muted, rgba(239, 68, 68, 0.15));
  border-color: rgba(239, 68, 68, 0.3);
  border-color: color-mix(in srgb, var(--nd-error, #EF4444) 30%, transparent);
  color: var(--nd-error, #EF4444);
}
.nd-badge--info {
  background: var(--nd-info-muted, rgba(59, 130, 246, 0.15));
  border-color: rgba(59, 130, 246, 0.3);
  border-color: color-mix(in srgb, var(--nd-info, #3B82F6) 30%, transparent);
  color: var(--nd-info, #3B82F6);
}

/* Table: the app's DataTable at its default (compact) density, as on the
   inventory page. 10px uppercase header on the surface, 28px rows on the app
   background, a rule between rows, the hover row on surface-hover, 16px
   outer cell padding. Cells stay on one line, as the app's do, so a row keeps
   its height. Cell text is the 14px body size. Wrap a table in
   \`.nd-table-wrap\` to scroll it sideways inside the wrapper instead of
   clipping. For a sortable column put a \`button\` in the \`th\` and set
   aria-sort on the \`th\`; the arrow follows aria-sort. \`.nd-num\` right
   aligns a numeric cell with tabular figures. */
.nd-table-wrap { max-width: 100%; overflow-x: auto; }
.nd-table {
  width: 100%;
  border-collapse: separate;
  border-spacing: 0;
  font-size: inherit;
}
.nd-table th {
  height: 33px;
  padding: 2px 8px;
  border-bottom: 1px solid var(--nd-border-subtle, #f3f4f6);
  background: var(--nd-surface, #ffffff);
  color: var(--nd-text-tertiary, #6b7280);
  font-size: 10px;
  font-weight: 600;
  line-height: 15px;
  letter-spacing: 0.05em;
  text-align: left;
  text-transform: uppercase;
  vertical-align: middle;
  white-space: nowrap;
}
.nd-table td {
  height: 28px;
  padding: 2px 8px;
  background: var(--nd-app-bg, #f3f4f6);
  color: var(--nd-text, #1f2937);
  vertical-align: middle;
  white-space: nowrap;
  transition: background-color 0.12s ease;
}
.nd-table tbody tr + tr td { border-top: 1px solid var(--nd-border, #e5e7eb); }
.nd-table tbody tr:hover td { background: var(--nd-surface-hover, #e5e7eb); }
.nd-table th:first-child, .nd-table td:first-child { padding-left: 16px; }
.nd-table th:last-child, .nd-table td:last-child { padding-right: 16px; }
.nd-table th > button {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  width: 100%;
  padding: 0;
  border: 0;
  background: none;
  font: inherit;
  color: inherit;
  letter-spacing: inherit;
  text-align: inherit;
  text-transform: inherit;
  cursor: pointer;
}
.nd-table th:has(> button):hover {
  background: var(--nd-surface-hover, #e5e7eb);
  color: var(--nd-text, #1f2937);
}
.nd-table th > button:focus-visible {
  outline: 2px solid var(--nd-accent, #FF6B1A);
  outline-offset: 2px;
  border-radius: 2px;
}
.nd-table th[aria-sort="ascending"] > button::after,
.nd-table th[aria-sort="descending"] > button::after,
.nd-table th[aria-sort="ascending"]:not(:has(> button))::after,
.nd-table th[aria-sort="descending"]:not(:has(> button))::after {
  color: var(--nd-text, #1f2937);
}
.nd-table th[aria-sort="ascending"] > button::after { content: "\\2191"; content: "\\2191" / ""; }
.nd-table th[aria-sort="descending"] > button::after { content: "\\2193"; content: "\\2193" / ""; }
.nd-table th[aria-sort="ascending"]:not(:has(> button))::after { content: "\\00a0\\2191"; content: "\\00a0\\2191" / ""; }
.nd-table th[aria-sort="descending"]:not(:has(> button))::after { content: "\\00a0\\2193"; content: "\\00a0\\2193" / ""; }
.nd-num { text-align: right; font-variant-numeric: tabular-nums; }
.nd-table th.nd-num { text-align: right; }
.nd-table th.nd-num > button { justify-content: flex-end; }

/* Tabs: the app's pill tab bar (recipes/tabs.ts, pill / sm, as on the asset
   and ticket headers). Put role="tablist" on the \`.nd-tabs\` element and
   role="tab" with aria-selected on each tab. */
.nd-tabs {
  display: flex;
  align-items: center;
  gap: 2px;
  width: fit-content;
  max-width: 100%;
  padding: 2px;
  overflow-x: auto;
  border-radius: var(--nd-radius-sm, 6px);
  background: var(--nd-surface-alt, #f3f4f6);
  scrollbar-width: none;
}
.nd-tabs [role="tab"] {
  font: inherit;
  display: inline-flex;
  flex-shrink: 0;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 10px;
  border: 0;
  border-radius: 4px;
  background: transparent;
  color: var(--nd-text-secondary, #4b5563);
  font-size: 12px;
  font-weight: 500;
  line-height: 16px;
  white-space: nowrap;
  text-decoration: none;
  cursor: pointer;
  transition: background-color 0.12s ease, color 0.12s ease;
}
.nd-tabs [role="tab"]:hover {
  background: rgba(255, 255, 255, 0.6);
  background: color-mix(in srgb, var(--nd-surface, #ffffff) 60%, transparent);
  color: var(--nd-text, #1f2937);
}
.nd-tabs [role="tab"][aria-selected="true"] {
  background: var(--nd-surface, #ffffff);
  color: var(--nd-text, #1f2937);
  box-shadow: 0 1px 3px 0 rgba(0, 0, 0, 0.1), 0 1px 2px -1px rgba(0, 0, 0, 0.1);
}
.nd-tabs [role="tab"]:focus-visible {
  outline: none;
  box-shadow: inset 0 0 0 2px var(--nd-accent, #FF6B1A);
}
.nd-tabs [role="tab"]:disabled, .nd-tabs [role="tab"][aria-disabled="true"] {
  opacity: 0.5;
  cursor: not-allowed;
}

/* Callout: the app's Callout. A neutral card with a 4px severity strip and a
   light wash of the severity colour; the title stays in the text colour so the
   strip carries the signal. As in the app, \`--info\` uses the accent colour.
   Optional \`.nd-callout-title\` for a heading line. */
.nd-callout {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 12px 16px 12px 20px;
  overflow: hidden;
  border: 1px solid var(--nd-border, #e5e7eb);
  border-radius: var(--nd-radius, 8px);
  background: var(--nd-surface, #ffffff);
  color: var(--nd-text-secondary, #4b5563);
}
.nd-callout::before {
  content: "";
  position: absolute;
  top: 0;
  bottom: 0;
  left: 0;
  width: 4px;
  background: var(--nd-border-strong, #d1d5db);
}
.nd-callout-title { font-weight: 500; color: var(--nd-text, #1f2937); }
.nd-callout--info { background: color-mix(in srgb, var(--nd-accent, #FF6B1A) 10%, var(--nd-surface, #ffffff)); }
.nd-callout--info::before { background: var(--nd-accent, #FF6B1A); }
.nd-callout--success { background: color-mix(in srgb, var(--nd-success, #00C951) 10%, var(--nd-surface, #ffffff)); }
.nd-callout--success::before { background: var(--nd-success, #00C951); }
.nd-callout--warning { background: color-mix(in srgb, var(--nd-warning, #F59E0B) 10%, var(--nd-surface, #ffffff)); }
.nd-callout--warning::before { background: var(--nd-warning, #F59E0B); }
.nd-callout--error { background: color-mix(in srgb, var(--nd-error, #EF4444) 10%, var(--nd-surface, #ffffff)); }
.nd-callout--error::before { background: var(--nd-error, #EF4444); }

/* Empty state: the app's EmptyState (compact) without the icon. Centred,
   muted; \`.nd-empty-title\` is the optional heading line. */
.nd-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding: 24px 12px;
  color: var(--nd-text-tertiary, #6b7280);
  font-size: 12px;
  text-align: center;
}
.nd-empty-title { color: var(--nd-text-secondary, #4b5563); font-size: 14px; font-weight: 500; }

/* Layout helpers. Flex with gap, never margins between children. \`.nd-stack\`
   is a column, \`.nd-row\` a wrapping centred row; both default to an 8px gap
   and take a \`.nd-gap-*\` modifier on the spacing tokens. \`.nd-grow\` takes the
   free space; \`.nd-push\` pushes itself and what follows to the far end. */
.nd-stack { display: flex; flex-direction: column; gap: var(--nd-space-sm, 8px); min-width: 0; }
.nd-row { display: flex; flex-flow: row wrap; align-items: center; gap: var(--nd-space-sm, 8px); min-width: 0; }
.nd-gap-xs { gap: var(--nd-space-xs, 4px); }
.nd-gap-sm { gap: var(--nd-space-sm, 8px); }
.nd-gap-md { gap: var(--nd-space-md, 12px); }
.nd-gap-lg { gap: var(--nd-space-lg, 16px); }
.nd-grow { flex: 1 1 0%; min-width: 0; }
.nd-push { margin-left: auto; }

/* Responsive contract
 *
 * READ THIS BEFORE WRITING A MEDIA QUERY. A plugin runs in its own document, so
 * a media query here resolves against the PANEL, not the app window. In a
 * ~336px sidebar panel, \`@media (min-width: 768px)\` is false on every display
 * ever made. That is container behaviour, and it is usually what you want, but
 * it is not what "min-width: 768px" suggests.
 *
 * Two attributes are stamped on \`<html>\`, plus \`--nd-container-width\`:
 *
 *   [data-nd-container]      narrow | medium | wide   how wide THIS panel is
 *   [data-nd-app-breakpoint] base | sm | md | lg | xl what the APP is doing
 *   [data-nd-pointer]        coarse | fine            touch or mouse
 *
 * \`data-nd-pointer\` is a convenience for JS; in CSS just write
 * \`@media (pointer: coarse)\`, which resolves correctly in here because pointer
 * is a device capability rather than a size.
 *
 * Lay the panel out against \`data-nd-container\` (or a plain media query, which
 * means the same thing). Reach for \`data-nd-app-breakpoint\` only to MATCH an
 * app-level decision, e.g. going flat because the app went to its stacked
 * mobile layout even though the panel itself did not get narrower.
 *
 *   [data-nd-container="narrow"] .my-grid { grid-template-columns: 1fr; }
 *   [data-nd-app-breakpoint="base"] .my-toolbar { display: none; }
 *
 * \`context.layout.breakpoint\` carries the app breakpoint for JS; the other two
 * are readable from the document itself.
 */

/* Touch targets. The app sizes its own controls to 44px under a coarse
 * pointer; the kit's controls follow so a plugin gets it without asking. */
@media (pointer: coarse) {
  .nd-btn, .nd-input, .nd-textarea { min-height: 44px; }
  .nd-select, .nd-tabs [role="tab"], .nd-table th > button { min-height: 44px; }
  /* Checkbox, radio and switch keep their drawn size; the 44px target comes
     from the label wrapping them (and, for the switch, a transparent hit
     area around the track), so the control does not balloon. */
  .nd-checkbox, .nd-radio { width: 20px; height: 20px; }
  label:has(> .nd-checkbox), label:has(> .nd-radio), label:has(> .nd-switch) { min-height: 44px; }
  .nd-switch::after { content: ""; position: absolute; inset: -12px -2px; }
}
`;
