import { describe, expect, it } from 'vitest'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { paletteForColor, SUPPORTED_COLOR_TOKENS } from '@nosdesk/core/utils/workflowColors'
import enUS from '../../../i18n/locales/en-US/main.ftl?raw'
import frFR from '../../../i18n/locales/fr-FR/main.ftl?raw'
import nlNL from '../../../i18n/locales/nl-NL/main.ftl?raw'

// The colours a new workspace's states are seeded with
// (`repository/workflow_states.rs`).
const SEEDED = ['slate', 'gray', 'blue', 'purple', 'green', 'subtle']

describe('workflow state colours', () => {
  it('offers every seeded colour in the picker', () => {
    for (const color of SEEDED) {
      expect(SUPPORTED_COLOR_TOKENS as readonly string[], color).toContain(color)
    }
  })

  it('renders each seeded colour differently', () => {
    expect(new Set(SEEDED.map((c) => paletteForColor(c).solid)).size).toBe(SEEDED.length)
    expect(new Set(SEEDED.map((c) => paletteForColor(c).badge)).size).toBe(SEEDED.length)
  })

  it('names each colour the picker offers in every catalogue', () => {
    for (const ftl of [enUS, frFR, nlNL]) {
      const bundle = new FluentBundle('en-US')
      bundle.addResource(new FluentResource(ftl))
      for (const color of SUPPORTED_COLOR_TOKENS) {
        expect(bundle.hasMessage(`admin-workflow-states-color-${color}`), color).toBe(true)
      }
    }
  })
})
