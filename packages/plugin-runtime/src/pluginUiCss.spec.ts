import { describe, it, expect } from 'vitest'
import { PLUGIN_UI_CSS } from './pluginUiCss'

// The kit's class names are a versioned plugin contract: removing or renaming
// one breaks every plugin that uses it. This list only ever grows.
const CONTRACT_CLASSES = [
  // Original set.
  'nd-btn',
  'nd-btn--primary',
  'nd-input',
  'nd-textarea',
  'nd-card',
  'nd-label',
  'nd-muted',
  // Buttons.
  'nd-btn--secondary',
  'nd-btn--danger',
  'nd-btn--ghost',
  'nd-btn--sm',
  // Form.
  'nd-select',
  'nd-checkbox',
  'nd-radio',
  'nd-switch',
  'nd-field',
  'nd-help',
  'nd-error',
  // Badge.
  'nd-badge',
  'nd-badge--neutral',
  'nd-badge--accent',
  'nd-badge--success',
  'nd-badge--warning',
  'nd-badge--error',
  'nd-badge--info',
  'nd-badge--no-dot',
  // Table.
  'nd-table',
  'nd-table-wrap',
  'nd-num',
  // Tabs, callout, empty state.
  'nd-tabs',
  'nd-callout',
  'nd-callout-title',
  'nd-callout--info',
  'nd-callout--success',
  'nd-callout--warning',
  'nd-callout--error',
  'nd-empty',
  'nd-empty-title',
  // Layout.
  'nd-stack',
  'nd-row',
  'nd-gap-xs',
  'nd-gap-sm',
  'nd-gap-md',
  'nd-gap-lg',
  'nd-grow',
  'nd-push',
]

// Every `--nd-*` name the host pushes: the TOKEN_MAP keys and the metric
// tokens in frontend/src/plugins/theme.ts. The kit may reference only these.
const HOST_TOKENS = new Set([
  'surface',
  'surface-alt',
  'surface-hover',
  'app-bg',
  'border',
  'border-subtle',
  'border-strong',
  'text',
  'text-secondary',
  'text-tertiary',
  'accent',
  'accent-hover',
  'on-accent',
  'success',
  'success-muted',
  'error',
  'error-muted',
  'warning',
  'warning-muted',
  'info',
  'info-muted',
  'font-sans',
  'font-mono',
  'radius',
  'radius-sm',
  'radius-lg',
  'space-xs',
  'space-sm',
  'space-md',
  'space-lg',
  'header-height',
  'header-padding-inline',
  'header-font-size',
  'header-font-weight',
])

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\-]/g, '\\$&')

describe('PLUGIN_UI_CSS', () => {
  it.each(CONTRACT_CLASSES)('defines the contract class .%s', (cls) => {
    // A selector use: the class followed by something that cannot continue
    // a class name, so `.nd-btn` is not satisfied by `.nd-btn--sm` alone.
    expect(PLUGIN_UI_CSS).toMatch(new RegExp(`\\.${escape(cls)}(?![\\w-])`))
  })

  it('references only token names the host pushes', () => {
    const used = new Set([...PLUGIN_UI_CSS.matchAll(/var\(--nd-([a-z-]+)/g)].map((m) => m[1]))
    const unknown = [...used].filter((name) => !HOST_TOKENS.has(name))
    expect(unknown).toEqual([])
  })

  it('gives every token reference a fallback for the first paint', () => {
    const bare = [...PLUGIN_UI_CSS.matchAll(/var\(--nd-[a-z-]+\)/g)].map((m) => m[0])
    expect(bare).toEqual([])
  })

  it('keeps the coarse-pointer 44px floor on every interactive control', () => {
    const coarse = PLUGIN_UI_CSS.slice(PLUGIN_UI_CSS.indexOf('@media (pointer: coarse)'))
    for (const sel of ['.nd-btn', '.nd-input', '.nd-textarea', '.nd-select', '[role="tab"]', 'th > button']) {
      expect(coarse).toContain(sel)
    }
    expect(coarse).toMatch(/label:has\(> \.nd-checkbox\)/)
    expect(coarse).toMatch(/\.nd-switch::after/)
  })

  it('has no em dashes', () => {
    expect(PLUGIN_UI_CSS).not.toContain('—')
  })
})
