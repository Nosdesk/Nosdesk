import { describe, expect, it } from 'vitest'
import { paletteForColor, SUPPORTED_COLOR_TOKENS } from '@nosdesk/core/utils/workflowColors'

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
})
